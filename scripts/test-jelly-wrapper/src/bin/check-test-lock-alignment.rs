use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

type Package = (String, String, String, String);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("test-jelly wrapper must live under <repo>/scripts/test-jelly-wrapper")
        .to_path_buf()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

fn flush_package(
    packages: &mut BTreeSet<Package>,
    name: &mut Option<String>,
    version: &mut Option<String>,
    source: &mut Option<String>,
    checksum: &mut Option<String>,
) {
    let Some(source_value) = source.take() else {
        *name = None;
        *version = None;
        *checksum = None;
        return;
    };
    if !source_value.starts_with("registry+") {
        *name = None;
        *version = None;
        *checksum = None;
        return;
    }
    if let (Some(name_value), Some(version_value)) = (name.take(), version.take()) {
        packages.insert((
            name_value,
            version_value,
            source_value,
            checksum.take().unwrap_or_default(),
        ));
    }
    *checksum = None;
}

fn registry_packages(path: &Path) -> Result<BTreeSet<Package>, String> {
    let input = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    let mut packages = BTreeSet::new();
    let mut name = None;
    let mut version = None;
    let mut source = None;
    let mut checksum = None;
    let mut in_package = false;

    for line in input.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if in_package {
                flush_package(
                    &mut packages,
                    &mut name,
                    &mut version,
                    &mut source,
                    &mut checksum,
                );
            }
            in_package = true;
            name = None;
            version = None;
            source = None;
            checksum = None;
            continue;
        }
        if !in_package || line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "name" => name = Some(unquote(value)),
                "version" => version = Some(unquote(value)),
                "source" => source = Some(unquote(value)),
                "checksum" => checksum = Some(unquote(value)),
                _ => {}
            }
        }
    }

    if in_package {
        flush_package(
            &mut packages,
            &mut name,
            &mut version,
            &mut source,
            &mut checksum,
        );
    }

    Ok(packages)
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn main() -> ExitCode {
    let root = repo_root();
    let root_lock = root.join("Cargo.lock");
    let probe_locks = [
        root.join("tests/suite/support/event-probe/Cargo.lock"),
        root.join("tests/suite/support/agent-api-probe/Cargo.lock"),
    ];

    let root_packages = match registry_packages(&root_lock) {
        Ok(packages) => packages,
        Err(error) => {
            eprintln!("check-test-lock-alignment: {error}");
            return ExitCode::from(2);
        }
    };

    let mut failed = false;
    for lock in probe_locks {
        let probe = match registry_packages(&lock) {
            Ok(packages) => packages,
            Err(error) => {
                eprintln!("FAIL {}", relative_display(&root, &lock));
                eprintln!("  {error}");
                failed = true;
                continue;
            }
        };

        let missing = root_packages.difference(&probe).collect::<Vec<_>>();
        let extra = probe.difference(&root_packages).collect::<Vec<_>>();
        if missing.is_empty() && extra.is_empty() {
            println!("PASS {}", relative_display(&root, &lock));
            continue;
        }

        failed = true;
        eprintln!("FAIL {}", relative_display(&root, &lock));
        for package in missing {
            eprintln!("  missing root package: {package:?}");
        }
        for package in extra {
            eprintln!("  extra/different package: {package:?}");
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
