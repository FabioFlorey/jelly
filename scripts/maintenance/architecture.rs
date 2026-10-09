//! Static architecture guardrails. Intentionally reads source; never runs it.
use super::Result;
use std::{fs, path::Path};
fn production(text: &str) -> &str {
    text.split("#[cfg(test)]\n").next().unwrap_or(text)
}
fn files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut result = vec![];
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            result.push(path)
        }
    }
    result.sort();
    Ok(result)
}
pub fn check(root: &Path) -> Result<()> {
    let mut failures = Vec::new();
    for path in files(&root.join("src/primitives"))? {
        let contents = fs::read_to_string(&path)?;
        for (index, line) in contents.lines().enumerate() {
            let line = line.trim();
            if line.contains("Err(\"") && line.contains(".into()")
                || line.contains("Err(format!(")
                || line.contains("ok_or(\"")
            {
                failures.push(format!(
                    "{}:{}: untyped primitive error",
                    path.display(),
                    index + 1
                ));
            }
            if line.contains("fs::")
                && (line.contains("read(")
                    || line.contains("read_to_string(")
                    || line.contains("write(")
                    || line.contains("create_dir_all(")
                    || line.contains("metadata(")
                    || line.contains("canonicalize(")
                    || line.contains("File::open("))
                && line.contains('?')
            {
                failures.push(format!(
                    "{}:{}: unclassified filesystem error",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    for name in [
        "surface.rs",
        "catalog_config.rs",
        "catalog_cache.rs",
        "catalog_build.rs",
        "catalog.rs",
    ] {
        let file = root.join("src/agent").join(name);
        let text = fs::read_to_string(&file)?;
        let text = production(&text);
        for banned in [
            "\"legacy\"",
            "\"compact\"",
            "Legacy",
            "Compact",
            "legacy_agent_catalog",
            "compact_agent_catalog",
        ] {
            if text.contains(banned) {
                failures.push(format!(
                    "{}: forbidden surface marker {banned}",
                    file.display()
                ))
            }
        }
    }
    let lib = fs::read_to_string(root.join("src/lib.rs"))?;
    for name in [
        "agent",
        "artifacts",
        "browser",
        "error",
        "execution",
        "mcp_auth",
        "primitives",
    ] {
        if !lib
            .lines()
            .any(|line| line.trim() == format!("mod {name};"))
            || lib
                .lines()
                .any(|line| line.trim() == format!("pub mod {name};"))
        {
            failures.push(format!("src/lib.rs: expected private module {name}"));
        }
    }
    for name in ["browser_launcher", "mcp", "recording", "routine"] {
        if !lib
            .lines()
            .any(|line| line.trim() == format!("pub mod {name};"))
        {
            failures.push(format!("src/lib.rs: expected public module {name}"));
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "cleanup architecture guardrails: FAIL\n{}",
            failures.join("\n")
        )
        .into());
    }
    println!("cleanup architecture guardrails: PASS (Rust)");
    Ok(())
}
