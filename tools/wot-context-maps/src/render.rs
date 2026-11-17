use crate::process;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path, process::Command};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

pub fn check_graphviz() -> Result<()> {
    let (status, _, stderr) = process::capture(Command::new("dot").arg("-V"))
        .context("Install Graphviz and put dot on PATH")?;
    ensure!(status.success(), "Graphviz failed to start: {stderr}");
    Ok(())
}

pub fn svg(graph: &Path, output: &Path) -> Result<Vec<u8>> {
    let source = fs::read_to_string(graph)
        .with_context(|| format!("Read generated graph {}", graph.display()))?;
    fs::write(graph, normalize_dot(&source))?;
    let (status, stdout, stderr) = process::capture(
        Command::new("dot")
            .arg("-Tsvg")
            .arg(graph)
            .arg("-o")
            .arg(output),
    )?;
    print!("{stdout}");
    eprint!("{stderr}");
    ensure!(
        status.success(),
        "Graphviz failed for {}: {status}",
        graph.display()
    );
    let svg = fs::read_to_string(output)
        .with_context(|| format!("Read rendered SVG {}", output.display()))?;
    validate_svg(&svg).with_context(|| format!("Invalid SVG at {}", output.display()))?;
    Ok(svg.into_bytes())
}

fn normalize_dot(source: &str) -> String {
    const ORIENTATION: &str = "\"orientation\"=\"";
    let mut normalized = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let contents = line.trim_end_matches(['\r', '\n']);
        // The generated graph references its temporary image directory and uses random egg angles.
        if contents
            .strip_prefix("graph [\"imagepath\"=\"")
            .and_then(|path| path.strip_suffix("\"]"))
            .is_some_and(|path| !path.contains('"'))
        {
            continue;
        }
        let mut remaining = line;
        while let Some((before, value)) = remaining.split_once(ORIENTATION) {
            let Some((_, after)) = value.split_once('"') else {
                break;
            };
            normalized.push_str(before);
            normalized.push_str(ORIENTATION);
            normalized.push_str("0\"");
            remaining = after;
        }
        normalized.push_str(remaining);
    }
    normalized
}

fn validate_svg(source: &str) -> Result<()> {
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )?;
    let tag = document.root_element().tag_name();
    ensure!(
        tag.name() == "svg" && tag.namespace() == Some(SVG_NAMESPACE),
        "Expected an SVG root element"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_context_mapper_noise_and_preserves_other_attributes() {
        let source = "digraph ContextMapGraph {\ngraph [\"imagepath\"=\"/tmp/x\"]\nnode [\"orientation\"=\"21.9\",\"width\"=\"2\"]\n}";
        let expected = "digraph ContextMapGraph {\nnode [\"orientation\"=\"0\",\"width\"=\"2\"]\n}";
        assert_eq!(normalize_dot(source), expected);
        assert_eq!(normalize_dot(expected), expected);
    }

    #[test]
    fn accepts_graphviz_doctype_and_requires_a_well_formed_svg() {
        let svg = r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg xmlns="http://www.w3.org/2000/svg"/>"#;
        assert!(validate_svg(svg).is_ok());
        assert!(validate_svg("<svg/>").is_err());
        assert!(validate_svg("<svg>").is_err());
    }
}
