//! Immutable saved-routine revisions and explicit active promotion.
use crate::core::routines::{valid_routine_name, validate_routine_definition};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

fn archive(root: &Path, name: &str) -> PathBuf {
    root.join(".versions").join(name)
}

fn archived(root: &Path, name: &str, revision: u64) -> PathBuf {
    archive(root, name).join(format!("v{revision}.json"))
}

fn active_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!("{name}.json"))
}

fn active(root: &Path, name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    if !valid_routine_name(name) {
        return Err("invalid routine name".into());
    }
    let graph: Value = serde_json::from_slice(&fs::read(active_path(root, name))?)?;
    validate_routine_definition(&graph, Some(name))?;
    Ok(graph)
}

pub(crate) fn revision(graph: &Value) -> Result<u64, Box<dyn std::error::Error>> {
    match graph.get("revision") {
        None => Ok(1),
        Some(value) => match value.as_u64() {
            Some(number @ 1..=1_000_000) => Ok(number),
            _ => Err("revision must be an integer from 1 to 1000000".into()),
        },
    }
}

fn numbered(mut graph: Value, revision: u64) -> Value {
    graph["revision"] = json!(revision);
    graph
}

fn lock(root: &Path, name: &str) -> Result<File, Box<dyn std::error::Error>> {
    if !valid_routine_name(name) {
        return Err("invalid routine name".into());
    }
    let dir = archive(root, name);
    fs::create_dir_all(&dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join(".lock"))?;
    file.try_lock()
        .map_err(|error| format!("routine version busy: {error}"))?;
    Ok(file)
}

fn temporary(dir: &Path, graph: &Value) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join(format!(".{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    let written = (|| {
        file.write_all(&serde_json::to_vec_pretty(graph)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&path);
    }
    written.map(|()| path)
}

/// Atomic create-new via hard link: a published archive file is never replaced.
fn archive_once(
    root: &Path,
    name: &str,
    number: u64,
    graph: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = temporary(&archive(root, name), graph)?;
    let result = fs::hard_link(&temp, archived(root, name, number));
    let _ = fs::remove_file(temp);
    result.map_err(Into::into)
}

fn archived_numbers(root: &Path, name: &str) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let mut numbers = Vec::new();
    let dir = archive(root, name);
    if !dir.exists() {
        return Ok(numbers);
    }
    for entry in fs::read_dir(dir)? {
        let file = entry?.file_name();
        let Some(file) = file.to_str() else { continue };
        if let Some(number) = file
            .strip_prefix('v')
            .and_then(|n| n.strip_suffix(".json"))
            .and_then(|n| n.parse::<u64>().ok())
            && number > 0
            && number <= 1_000_000
        {
            numbers.push(number);
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    Ok(numbers)
}

pub(crate) fn stage(
    root: &Path,
    name: &str,
    expected: u64,
    mut candidate: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let _guard = lock(root, name)?;
    let current = active(root, name)?;
    let active_number = revision(&current)?;
    if expected != active_number {
        return Err("active routine revision changed".into());
    }
    if candidate.get("revision").is_some() {
        return Err("omit revision from candidate; Jelly assigns it".into());
    }
    if candidate.get("name") != current.get("name") {
        return Err("cannot rename a routine while revising".into());
    }
    if candidate.get("id").is_some() && candidate.get("id") != current.get("id") {
        return Err("cannot change routine UUID".into());
    }
    if let Some(id) = current.get("id") {
        candidate["id"] = id.clone();
    }
    let original = numbered(current.clone(), active_number);
    let original_path = archived(root, name, active_number);
    if original_path.exists() {
        let existing: Value = serde_json::from_slice(&fs::read(original_path)?)?;
        if existing != original {
            return Err("active routine differs from archived revision".into());
        }
    } else {
        archive_once(root, name, active_number, &original)?;
    }
    let next = archived_numbers(root, name)?
        .last()
        .copied()
        .unwrap_or(active_number)
        .max(active_number)
        .checked_add(1)
        .filter(|n| *n <= 1_000_000)
        .ok_or("routine revision limit reached")?;
    candidate["revision"] = json!(next);
    validate_routine_definition(&candidate, Some(name))?;
    if serde_json::to_vec(&candidate)?.len() > 64 * 1024 {
        return Err("routine exceeds 64 KiB".into());
    }
    archive_once(root, name, next, &candidate)?;
    Ok(
        json!({"status":"staged","name":name,"id":candidate.get("id"),
        "revision":next,"active_revision":active_number}),
    )
}

pub(crate) fn exact(
    root: &Path,
    name: &str,
    number: u64,
) -> Result<Value, Box<dyn std::error::Error>> {
    if !valid_routine_name(name) || !(1..=1_000_000).contains(&number) {
        return Err("invalid routine name or revision".into());
    }
    let graph: Value = match fs::read(archived(root, name, number)) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let current = active(root, name)?;
            if revision(&current)? != number {
                return Err("routine revision not found".into());
            }
            current
        }
        Err(error) => return Err(error.into()),
    };
    validate_routine_definition(&graph, Some(name))?;
    if revision(&graph)? != number {
        return Err("archive revision mismatch".into());
    }
    Ok(graph)
}

pub(crate) fn promote(
    root: &Path,
    name: &str,
    number: u64,
    expected: u64,
) -> Result<Value, Box<dyn std::error::Error>> {
    let _guard = lock(root, name)?;
    let current = active(root, name)?;
    let was = revision(&current)?;
    if expected != was {
        return Err("active routine revision changed".into());
    }
    if was == number {
        return Err("revision already active".into());
    }
    let next = exact(root, name, number)?;
    if next.get("id") != current.get("id") {
        return Err("routine UUID changed".into());
    }
    let temp = temporary(root, &next)?;
    let result = fs::rename(&temp, active_path(root, name));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result?;
    Ok(json!({"status":"promoted","name":name,"id":next.get("id"),
        "revision":number,"previous_revision":was}))
}

pub(crate) fn versions(root: &Path, name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let current = active(root, name)?;
    let live = revision(&current)?;
    let mut numbers = archived_numbers(root, name)?;
    if !numbers.contains(&live) {
        numbers.push(live);
    }
    numbers.sort_unstable();
    let mut versions = Vec::new();
    for number in numbers {
        let graph = exact(root, name, number)?;
        versions.push(json!({"revision":number,"active":number == live,
            "description":graph["description"],
            "mode":graph.get("mode").and_then(Value::as_str).unwrap_or("auto")}));
    }
    Ok(json!({"name":name,"id":current.get("id"),"active_revision":live,"versions":versions}))
}

pub(crate) fn pinned(identifier: &str) -> Option<(&str, u64)> {
    let (name, text) = identifier.rsplit_once("@v")?;
    if !valid_routine_name(name) || text.is_empty() || text.starts_with('0') {
        return None;
    }
    let number = text.parse::<u64>().ok()?;
    (number <= 1_000_000).then_some((name, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);
    impl TestRoot {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "jelly-versions-test-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn graph(description: &str) -> Value {
        json!({"name":"sample","description":description,"entry":"done",
            "nodes":{"done":{"terminal":"success"}}})
    }

    #[test]
    fn revisions_are_immutable_and_promotion_is_explicit() {
        let root = TestRoot::new();
        fs::write(
            active_path(&root.0, "sample"),
            serde_json::to_vec(&graph("first")).unwrap(),
        )
        .unwrap();
        assert_eq!(revision(&active(&root.0, "sample").unwrap()).unwrap(), 1);
        assert_eq!(
            stage(&root.0, "sample", 1, graph("second")).unwrap()["revision"],
            2
        );
        assert_eq!(active(&root.0, "sample").unwrap()["description"], "first");
        assert_eq!(exact(&root.0, "sample", 1).unwrap()["description"], "first");
        assert_eq!(
            exact(&root.0, "sample", 2).unwrap()["description"],
            "second"
        );
        assert!(stage(&root.0, "sample", 0, graph("invalid")).is_err());
        assert!(promote(&root.0, "sample", 2, 2).is_err());
        promote(&root.0, "sample", 2, 1).unwrap();
        assert_eq!(versions(&root.0, "sample").unwrap()["active_revision"], 2);
        assert_eq!(
            stage(&root.0, "sample", 2, graph("third")).unwrap()["revision"],
            3
        );
        promote(&root.0, "sample", 1, 2).unwrap();
        assert_eq!(active(&root.0, "sample").unwrap()["description"], "first");
        assert_eq!(exact(&root.0, "sample", 3).unwrap()["description"], "third");
    }

    #[test]
    fn pinned_identifiers_reject_ambiguous_or_nonexistent_revisions() {
        assert_eq!(pinned("sample@v2"), Some(("sample", 2)));
        for bad in [
            "sample@v0",
            "sample@v01",
            "sample@v-1",
            "sample@vfoo",
            "sample@v1000001",
        ] {
            assert!(pinned(bad).is_none());
        }
        let root = TestRoot::new();
        fs::write(
            active_path(&root.0, "sample"),
            serde_json::to_vec(&graph("first")).unwrap(),
        )
        .unwrap();
        assert!(exact(&root.0, "sample", 90).is_err());
    }
}
