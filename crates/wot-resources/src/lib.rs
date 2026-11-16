#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const NATIONS: &[&str] = &[
    "ussr", "germany", "usa", "china", "france", "uk", "japan", "czech", "sweden", "poland",
    "italy",
];

const MANIFEST: &str = "manifest.json";
const FORMAT_VERSION: u32 = 1;

/// A read-only view of an extracted resource bundle.
#[derive(Clone)]
pub struct ResourceBundle {
    root: PathBuf,
    manifest: Arc<Manifest>,
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    format_version: u32,
    component_client_classes: BTreeSet<String>,
}

impl ResourceBundle {
    pub fn open(root: &Path) -> io::Result<Self> {
        let path = root.join(MANIFEST);
        let file = File::open(&path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Open {}: {error}. Generate a resource bundle with wot-extract.",
                    path.display()
                ),
            )
        })?;
        let manifest: Manifest =
            serde_json::from_reader(BufReader::new(file)).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Read {}: {error}", path.display()),
                )
            })?;
        if manifest.format_version != FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Unsupported bundle format {} at {} (expected {FORMAT_VERSION}). Regenerate with the matching wot-extract.",
                    manifest.format_version,
                    path.display()
                ),
            ));
        }
        Ok(Self {
            root: root.join("res"),
            manifest: Arc::new(manifest),
        })
    }

    /// Write the manifest after the extracted files are ready.
    pub fn write_manifest(
        root: &Path,
        component_client_classes: BTreeSet<String>,
    ) -> io::Result<()> {
        let mut file = BufWriter::new(
            File::options()
                .write(true)
                .create_new(true)
                .open(root.join(MANIFEST))?,
        );
        serde_json::to_writer_pretty(
            &mut file,
            &Manifest {
                format_version: FORMAT_VERSION,
                component_client_classes,
            },
        )?;
        file.write_all(b"\n")?;
        file.flush()
    }

    pub fn read(&self, path: impl AsRef<str>) -> io::Result<File> {
        let path = path.as_ref();
        File::open(self.resolve(path)?)
            .map_err(|error| io::Error::new(error.kind(), format!("{path}: {error}")))
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.resolve(path).is_ok_and(|path| path.is_file())
    }

    pub fn has_component_client_class(&self, name: &str) -> bool {
        self.manifest.component_client_classes.contains(name)
    }

    pub fn read_dir(&self, path: &str) -> io::Result<fs::ReadDir> {
        fs::read_dir(self.resolve(path)?)
    }

    fn resolve(&self, path: &str) -> io::Result<PathBuf> {
        if path.is_empty() {
            return Ok(self.root.clone());
        }
        validate_resource_path(path)?;
        Ok(self.root.join(path))
    }
}

/// Resource keys use slash-separated relative paths on every platform.
pub fn validate_resource_path(path: &str) -> io::Result<()> {
    if path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part.ends_with(['.', ' ']))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid resource path {path:?}"),
        ));
    }
    Ok(())
}
