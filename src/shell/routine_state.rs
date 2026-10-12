//! Filesystem-backed routine continuations. An advisory file lock serializes
//! resume by ID across processes; completion receipts make retries idempotent.
use crate::ROUTINE_STATE_DIR;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

fn root() -> PathBuf {
    ROUTINE_STATE_DIR.path().to_path_buf()
}

pub(crate) fn validate_resume_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid routine resume ID");
    }
    Ok(())
}

fn path(id: &str, suffix: &str) -> PathBuf {
    root().join(format!("{id}.{suffix}"))
}

/// Retain the lock file; deleting it while other processes hold an open file
/// descriptor would allow concurrent workers to lock different inodes.
pub(crate) fn acquire(id: &str) -> Result<File, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(root())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path(id, "lock"))?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => "routine resume already in progress".into(),
        std::fs::TryLockError::Error(error) => Box::new(error) as Box<dyn std::error::Error>,
    })?;
    Ok(file)
}

/// Atomic replacement with owner-only file permissions. The unique temporary
/// file and same-directory rename prevent observers from reading partial JSON.
pub(crate) fn save_checkpoint(id: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(root())?;
    let temp = root().join(format!("{id}.{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&temp, path(id, "state"))?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub(crate) fn checkpoint(id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    Ok(serde_json::from_slice(&fs::read(path(id, "state"))?)?)
}

pub(crate) fn receipt(id: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    match fs::read(path(id, "result")) {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// A receipt is written *before* removing the checkpoint; retries after a crash
/// between these two steps will return the receipt, never re-execute actions.
pub(crate) fn record(id: &str, result: &Value) -> Result<(), Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(root())?;
    if path(id, "result").exists() {
        return Err("routine completion receipt already exists".into());
    }
    let temp = root().join(format!("{id}.{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let written = (|| {
        file.write_all(&serde_json::to_vec_pretty(result)?)?;
        file.sync_all()?;
        fs::rename(&temp, path(id, "result"))?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(temp);
    }
    written
}

pub(crate) fn inspect(id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    if !path(id, "state").exists() && !path(id, "result").exists() {
        return Err("routine continuation not found".into());
    }
    let _lock = match acquire(id) {
        Ok(file) => file,
        Err(error) if error.to_string() == "routine resume already in progress" => {
            return Ok(json!({"status":"running","resume_id":id}));
        }
        Err(error) => return Err(error),
    };
    if let Some(receipt) = receipt(id)? {
        return Ok(json!({
            "status":receipt["status"],"resume_id":id,
            "terminal":true
        }));
    }
    let state = checkpoint(id)?;
    if state["kind"] != "graph" {
        return Err("routine continuation is not a JSON graph".into());
    }
    Ok(json!({
        "status":if state["inflight"] == true { "interrupted" } else { "suspended" },
        "resume_id":id,
        "handoff_to":state["handoff_to"].as_str().unwrap_or("caller"),
        "pending_child":state["pending_child"],
        "awaiting":state["awaiting"],
        "current":state["current"]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_ids_are_numeric_and_bounded() {
        for id in ["", "abc", "../state", "12.4", "1".repeat(41).as_str()] {
            assert!(validate_resume_id(id).is_err());
        }
        assert!(validate_resume_id("1234").is_ok());
    }

    #[test]
    fn checkpoint_and_receipt_have_distinct_inspectable_states() {
        let id = format!("998{:018}", rand::random::<u64>());
        let mut state = json!({
            "kind":"graph","current":"verify","handoff_to":"caller",
            "awaiting":{"kind":"agent","instruction":"Inspect invoice"},"inflight":false
        });
        save_checkpoint(&id, &state).unwrap();
        assert_eq!(inspect(&id).unwrap()["status"], "suspended");
        assert_eq!(
            inspect(&id).unwrap()["awaiting"]["instruction"],
            "Inspect invoice"
        );

        let held = acquire(&id).unwrap();
        assert_eq!(inspect(&id).unwrap()["status"], "running");
        drop(held);

        state["inflight"] = json!(true);
        save_checkpoint(&id, &state).unwrap();
        assert_eq!(inspect(&id).unwrap()["status"], "interrupted");

        let terminal = json!({"status":"completed","context":{"confidential":"private"}});
        record(&id, &terminal).unwrap();
        let summary = inspect(&id).unwrap();
        assert_eq!(summary["status"], "completed");
        assert_eq!(summary["terminal"], true);
        assert!(summary.get("context").is_none());
        assert_eq!(receipt(&id).unwrap(), Some(terminal));

        let _ = fs::remove_file(path(&id, "state"));
        let _ = fs::remove_file(path(&id, "result"));
        let _ = fs::remove_file(path(&id, "lock"));
    }

    #[test]
    fn resume_locks_prevent_concurrent_workers() {
        let id = format!("999{:018}", rand::random::<u64>());
        let first = acquire(&id).unwrap();
        let second = acquire(&id).unwrap_err();
        assert!(second.to_string().contains("already in progress"));
        drop(first);
        let third = acquire(&id).unwrap();
        drop(third);
        // The test owns this unique ID and has released every descriptor.
        let _ = fs::remove_file(path(&id, "lock"));
    }
}
