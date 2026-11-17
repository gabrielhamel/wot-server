use crate::process;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::Path,
    process::Command,
    time::Duration,
};

const VERSION: &str = "6.12.0";
const SHA256: &str = "1a3e2a6a48b428c32831c8f43e6152f45e2538836bad8852cea7e0ca6c04f6b6";
const JAVA_CLASS: &str = "org.contextmapper.cli.ContextMapperCLI";
const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;

pub struct Mapper {
    java: OsString,
    classpath: OsString,
}

impl Mapper {
    pub fn install(work: &Path) -> Result<Self> {
        let java = env::var_os("JAVA_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(
                || OsString::from("java"),
                |home| {
                    Path::new(&home)
                        .join("bin")
                        .join(format!("java{}", env::consts::EXE_SUFFIX))
                        .into_os_string()
                },
            );
        let (status, _, stderr) = process::capture(Command::new(&java).arg("-version"))
            .context("Install Java 17+ or set JAVA_HOME to its installation")?;
        ensure!(status.success(), "Java failed to start: {stderr}");

        let classpath_dir = work.join("lib");
        fs::create_dir(&classpath_dir)?;
        let archive_name = format!("context-mapper-cli-{VERSION}");
        let url = format!(
            "https://repo.maven.apache.org/maven2/org/contextmapper/context-mapper-cli/{VERSION}/{archive_name}.tar"
        );
        let mut bytes = Vec::new();
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?
            .get(url)
            .send()
            .context("Download the pinned Context Mapper CLI")?
            .error_for_status()?
            .take(ARCHIVE_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= ARCHIVE_LIMIT,
            "Context Mapper archive is too large"
        );
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ensure!(
            hash == SHA256,
            "Context Mapper CLI archive checksum does not match"
        );
        extract_jars(
            &bytes,
            &Path::new(&archive_name).join("lib"),
            &classpath_dir,
        )?;
        Ok(Self {
            java,
            classpath: classpath_dir.join("*").into_os_string(),
        })
    }

    pub fn validate(&self, model: &Path) -> Result<()> {
        let stdout = self.invoke("validate", model, None)?;
        // This Context Mapper version reports certain validation errors with exit status zero.
        ensure!(
            stdout.contains("has been validated without errors."),
            "CML validation failed: {}",
            model.display()
        );
        Ok(())
    }

    pub fn generate(&self, model: &Path, output: &Path) -> Result<()> {
        self.invoke("generate", model, Some(output))?;
        Ok(())
    }

    fn invoke(&self, action: &str, model: &Path, output: Option<&Path>) -> Result<String> {
        let mut command = Command::new(&self.java);
        command
            .args(["-cp"])
            .arg(&self.classpath)
            .args([JAVA_CLASS, action, "-i"])
            .arg(model);
        if let Some(output) = output {
            command.args(["-g", "context-map", "-o"]).arg(output);
        }
        let (status, stdout, stderr) = process::capture(&mut command)?;
        print!("{stdout}");
        eprint!("{stderr}");
        ensure!(
            status.success(),
            "Context Mapper {action} failed for {}: {status}",
            model.display()
        );
        Ok(stdout)
    }
}

fn extract_jars(archive: &[u8], lib_path: &Path, destination: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(archive);
    let mut extracted = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if !entry.header().entry_type().is_file()
            || path.parent() != Some(lib_path)
            || path.extension().is_none_or(|extension| extension != "jar")
        {
            continue;
        }
        let name = path
            .file_name()
            .context("Missing JAR filename in Context Mapper archive")?;
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(destination.join(name))?;
        io::copy(&mut entry, &mut output)?;
        extracted = true;
    }
    ensure!(extracted, "No Context Mapper JARs found in the archive");
    Ok(())
}
