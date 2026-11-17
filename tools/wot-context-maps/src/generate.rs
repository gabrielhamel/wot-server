use crate::{mapper::Mapper, render};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::Path,
};

pub fn run(models_dir: &Path) -> Result<()> {
    let mut models = Vec::new();
    for entry in fs::read_dir(models_dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "cml") && path.is_file() {
            models.push(path);
        }
    }
    models.sort_unstable();
    ensure!(
        !models.is_empty(),
        "No CML models found in {}",
        models_dir.display()
    );
    render::check_graphviz()?;
    let work = tempfile::Builder::new()
        .prefix("wot-context-maps-")
        .tempdir()?;
    let mapper = Mapper::install(work.path())?;
    let temporary_output = work.path().join("generated");
    fs::create_dir(&temporary_output)?;
    let mut rendered = BTreeMap::new();
    for model in models {
        mapper.validate(&model)?;
        mapper.generate(&model, &temporary_output)?;
        let stem = model.file_stem().context("Missing CML model name")?;
        let graph = temporary_output.join(append(stem, "_ContextMap.gv"));
        let svg = temporary_output.join(append(stem, "_ContextMap.svg"));
        rendered.insert(append(stem, ".svg"), render::svg(&graph, &svg)?);
    }
    let destination = models_dir.join("generated");
    publish(&destination, &rendered)?;
    println!(
        "Generated {} context maps in {}",
        rendered.len(),
        destination.display()
    );
    Ok(())
}

fn append(stem: &OsStr, suffix: &str) -> OsString {
    let mut name = stem.to_os_string();
    name.push(suffix);
    name
}

fn publish(destination: &Path, rendered: &BTreeMap<OsString, Vec<u8>>) -> Result<()> {
    fs::create_dir_all(destination)?;
    for (name, bytes) in rendered {
        let target = destination.join(name);
        if fs::read(&target).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        let mut pending = tempfile::NamedTempFile::new_in(destination)?;
        pending.write_all(bytes)?;
        pending
            .persist(&target)
            .with_context(|| format!("Publish {}", target.display()))?;
    }
    for old in fs::read_dir(destination)? {
        let old = old?;
        let path = old.path();
        if path.extension().is_some_and(|extension| extension == "svg")
            && (old.file_type()?.is_file() || old.file_type()?.is_symlink())
            && !rendered.contains_key(&old.file_name())
        {
            fs::remove_file(&path).with_context(|| format!("Remove {}", path.display()))?;
        }
    }
    Ok(())
}
