//! Offline Markdown and documentation validation.
use super::Result;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn md_files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            md_files(&path, output)?
        } else if path.extension().is_some_and(|e| e == "md") {
            output.push(path)
        }
    }
    Ok(())
}
fn slug(header: &str) -> String {
    let mut content = String::new();
    let mut in_tag = false;
    for c in header.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag && c != '`' && (c.is_alphanumeric() || c == ' ' || c == '-' || c == '_') {
            content.push(c)
        }
    }
    content
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}
fn anchors(text: &str) -> HashSet<String> {
    let mut result = HashSet::new();
    let mut counts = HashMap::<String, usize>::new();
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("id=\"")
            && let Some(id) = rest.split('"').next()
        {
            result.insert(id.to_owned());
        }
        if let Some((hashes, tail)) = line.split_once(' ')
            && (1..=6).contains(&hashes.len())
            && hashes.bytes().all(|b| b == b'#')
        {
            let name = slug(tail.trim().trim_end_matches('#').trim());
            let suffix = counts.entry(name.clone()).or_default();
            result.insert(if *suffix == 0 {
                name
            } else {
                format!("{name}-{suffix}")
            });
            *suffix += 1;
        }
    }
    result
}
fn links(text: &str) -> Vec<String> {
    let mut result = vec![];
    // Extract Markdown destinations and HTML hrefs, preserving fragments.
    for (prefix, end) in [("](", ")"), ("href=\"", "\"")] {
        let mut rest = text;
        while let Some(pos) = rest.find(prefix) {
            rest = &rest[pos + prefix.len()..];
            if let Some(last) = rest.find(end) {
                let url = rest[..last].trim();
                if !url.contains('\n') {
                    result.push(url.to_owned())
                }
                rest = &rest[last + end.len()..];
            } else {
                break;
            }
        }
    }
    result
}
fn decode_path(input: &str) -> String {
    let mut bytes = Vec::new();
    let input = input.as_bytes();
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%'
            && i + 2 < input.len()
            && let Ok(hex) = u8::from_str_radix(&String::from_utf8_lossy(&input[i + 1..i + 3]), 16)
        {
            bytes.push(hex);
            i += 3;
            continue;
        }
        bytes.push(input[i]);
        i += 1
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
fn snippets(source: &str) -> Vec<(String, String)> {
    let mut snippets = Vec::new();
    let mut lines = source.lines();
    while let Some(line) = lines.next() {
        if let Some(lang) = line.trim_start().strip_prefix("```") {
            if lang.starts_with('`') {
                continue;
            }
            let mut content = String::new();
            let mut closed = false;
            for item in lines.by_ref() {
                if item.trim() == "```" {
                    closed = true;
                    break;
                }
                content.push_str(item);
                content.push('\n')
            }
            if closed {
                snippets.push((lang.trim().to_owned(), content))
            }
        }
    }
    snippets
}
fn bash_syntax(source: &str) -> Result<bool> {
    let mut child = Command::new("bash")
        .args(["-n", "-c", source])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child.wait()?.success())
}
pub fn check(root: &Path) -> Result<()> {
    let docs = root.join("docs");
    let index = docs.join("INDEX.md");
    let mut files = vec![];
    md_files(&docs, &mut files)?;
    files.sort();
    if files.len() < 12 || !files.contains(&index) {
        return Err("missing required documentation".into());
    }
    let readme = fs::read_to_string(root.join("README.md"))?;
    let mut failed = vec![];
    for required in [
        "<img src=\"./assets/full-logo.png\" alt=\"Jelly\" width=\"760\">",
        "[**Quickstart**](#3-quickstart)",
        "[**Documentation**](#8-documentation-and-support)",
        "[documentation index](./docs/INDEX.md)",
        "assets/porsche-718-spyder-rs-demo-20261008.gif",
        "Model Context Protocol (MCP)",
        "**Browser instrumentation for agents**",
    ] {
        if !readme.contains(required) {
            failed.push(format!(
                "README.md: required hero/navigation content missing: {required}"
            ))
        }
    }
    if readme.matches("<div align=\"center\">").count() < 2 {
        failed.push("README: two center blocks required".into())
    }
    for badge in [
        "Stars",
        "Forks",
        "CI",
        "Rust 1.98.1",
        "License: Proprietary",
    ] {
        if readme.matches(&format!("[![{badge}]")).count() != 1 {
            failed.push(format!("README badge {badge} absent/duplicated"))
        }
    }
    if !fs::read_to_string(root.join("SECURITY.md"))?.contains("raw_cdp = true") {
        failed.push("SECURITY.md: raw-CDP setting missing".into())
    }
    let mut indexed = HashSet::new();
    let mut local_links = 0usize;
    let mut examples = HashMap::new();
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(root.join("config/jelly.toml"))?)?;
    let mut tech_example = None;
    for path in std::iter::once(root.join("README.md")).chain(files.iter().cloned()) {
        let text = fs::read_to_string(&path)?;
        let display = path.strip_prefix(root).unwrap_or(&path).display();
        if text.contains('—') {
            failed.push(format!("{display}: em dash violates writing style"))
        }
        if path != root.join("README.md") {
            if !text
                .lines()
                .any(|line| line.starts_with("# ") && !line.starts_with("# ##"))
            {
                failed.push(format!("{display}: missing heading"))
            }
            if path != index
                && path.file_name().is_none_or(|s| s != "GLOSSARY.md")
                && !text.contains("Documentation index")
            {
                failed.push(format!("{display}: missing documentation index link"))
            }
        }
        for reference in links(&text) {
            if reference.starts_with('#')
                || (!reference.contains("://")
                    && !reference.starts_with("mailto:")
                    && !reference.starts_with("//"))
            {
                let target_text = reference.split('?').next().unwrap_or(&reference);
                let (location, anchor) = target_text.split_once('#').unwrap_or((target_text, ""));
                let target = if location.is_empty() {
                    path.clone()
                } else {
                    path.parent().unwrap().join(decode_path(location))
                };
                if !target.exists() {
                    failed.push(format!("{display}: missing link {reference}"));
                    continue;
                }
                local_links += 1;
                if !anchor.is_empty()
                    && target.is_file()
                    && !anchors(&fs::read_to_string(&target)?).contains(&decode_path(anchor))
                {
                    failed.push(format!("{display}: missing anchor {reference}"));
                }
                if path == index && files.contains(&target) {
                    indexed.insert(target);
                }
            }
        }
        for (lang, content) in snippets(&text) {
            match lang.as_str() {
                "json" => {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                        failed.push(format!("{display}: invalid JSON: {e}"))
                    }
                }
                "toml" => match toml::from_str::<toml::Value>(&content) {
                    Ok(t) => {
                        if path == docs.join("getting-started/CONFIGURATION.md")
                            && content.contains("[paths]")
                        {
                            tech_example = Some(t)
                        }
                    }
                    Err(e) => failed.push(format!("{display}: invalid TOML: {e}")),
                },
                "bash" => {
                    if !bash_syntax(&content)? {
                        failed.push(format!("{display}: invalid Bash snippet"))
                    }
                }
                "mermaid" => {
                    if ![
                        "flowchart",
                        "graph",
                        "sequenceDiagram",
                        "stateDiagram",
                        "stateDiagram-v2",
                        "classDiagram",
                        "erDiagram",
                    ]
                    .iter()
                    .any(|prefix| content.trim_start().starts_with(prefix))
                    {
                        failed.push(format!("{display}: invalid Mermaid declaration"))
                    }
                }
                _ => continue,
            }
            *examples.entry(lang).or_insert(0usize) += 1;
        }
    }
    for file in &files {
        if file != &index && !indexed.contains(file) {
            failed.push(format!("docs index doesn't list {}", file.display()))
        }
    }
    if let Some(example) = tech_example {
        if let Some(table) = example.as_table() {
            for (section, keys) in table {
                for (key, expected) in keys.as_table().ok_or("bad config example")? {
                    if config.get(section).and_then(|t| t.get(key)) != Some(expected) {
                        failed.push(format!("config drift {section}.{key}"))
                    }
                }
            }
        }
    } else {
        failed.push("missing technical TOML example".into())
    }
    let runtime = fs::read_to_string(docs.join("reference/RUNTIME.md"))?;
    if !["--build", "Cargo build", "OAuth", "extensions"]
        .iter()
        .all(|v| runtime.contains(v))
    {
        failed.push("runtime cleanup documentation misses warnings".into())
    }
    println!(
        "Checked {} documents and README.md, {local_links} local links, {} JSON, {} TOML, {} Mermaid blocks, and {} Bash syntax checks",
        files.len(),
        examples.get("json").copied().unwrap_or(0),
        examples.get("toml").copied().unwrap_or(0),
        examples.get("mermaid").copied().unwrap_or(0),
        examples.get("bash").copied().unwrap_or(0)
    );
    if !failed.is_empty() {
        return Err(format!("documentation validation failed:\n{}", failed.join("\n")).into());
    }
    println!("PASS: Documentation structure, links, anchors and examples (Rust).");
    Ok(())
}
