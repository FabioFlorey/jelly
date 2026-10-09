pub use crate::core::downloads::DownloadRecord;
use crate::core::downloads::{
    CollisionPolicy, DOWNLOAD_STATE_VERSION, DownloadState, FinalizeState, MAX_DOWNLOAD_RECORDS,
    MaterializationPlan, Progress, WaitState, apply_progress, finalize_state, finalized_record,
    has_in_progress, interrupt_record, new_record, numbered_path, plan_materialization,
    request_cancel, wait_state,
};
use crate::{
    BROWSER_PID, BrowserSession, DOWNLOAD_DIR, Error, ErrorKind, STATE_DIR, jelly_error,
    register_download_with_context, sanitize_url,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{Message, WebSocket, connect};

static TEMP_NONCE: AtomicU64 = AtomicU64::new(1);

fn state_path() -> PathBuf {
    PathBuf::from(STATE_DIR.as_str()).join("downloads.json")
}

fn lock_path() -> PathBuf {
    PathBuf::from(STATE_DIR.as_str()).join("downloads.lock")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn private_file(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

struct StateLock {
    file: File,
}

impl StateLock {
    fn acquire() -> Result<Self, Error> {
        fs::create_dir_all(STATE_DIR)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path())?;
        file.lock()?;
        Ok(Self { file })
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn read_state_unlocked() -> Result<DownloadState, Error> {
    let path = state_path();
    if !path.exists() {
        return Ok(DownloadState::default());
    }
    let state: DownloadState = serde_json::from_slice(&fs::read(&path)?).map_err(|error| {
        jelly_error(
            ErrorKind::DownloadFailed,
            format!(
                "invalid persisted download state {}: {error}",
                path.display()
            ),
            false,
        )
    })?;
    if state.version != DOWNLOAD_STATE_VERSION {
        return Err(jelly_error(
            ErrorKind::DownloadFailed,
            format!(
                "unsupported download state version {} (expected {DOWNLOAD_STATE_VERSION})",
                state.version
            ),
            false,
        ));
    }
    Ok(state)
}

fn write_state_unlocked(state: &DownloadState) -> Result<(), Error> {
    let path = state_path();
    let parent = path.parent().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "download state path has no parent",
            false,
        )
    })?;
    fs::create_dir_all(parent)?;
    let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("json.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<(), Error> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(state)?)?;
        file.sync_all()?;
        private_file(&temp)?;
        fs::rename(&temp, &path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn with_state_mut<T>(f: impl FnOnce(&mut DownloadState) -> Result<T, Error>) -> Result<T, Error> {
    let _lock = StateLock::acquire()?;
    let mut state = read_state_unlocked()?;
    let output = f(&mut state)?;
    if state.downloads.len() > MAX_DOWNLOAD_RECORDS {
        state
            .downloads
            .sort_by_key(|record| std::cmp::Reverse(record.updated_at_ms));
        state.downloads.truncate(MAX_DOWNLOAD_RECORDS);
    }
    write_state_unlocked(&state)?;
    Ok(output)
}

fn browser_launcher_alive() -> bool {
    let pid = match fs::read_to_string(BROWSER_PID) {
        Ok(value) => value.trim().parse::<u32>().ok(),
        Err(_) => None,
    };
    let Some(pid) = pid else {
        return false;
    };
    let proc = PathBuf::from(format!("/proc/{pid}"));
    if !proc.is_dir() {
        return false;
    }
    fs::read(proc.join("cmdline"))
        .ok()
        .is_some_and(|cmdline| String::from_utf8_lossy(&cmdline).contains("agent-open-browser"))
}

fn reconcile_orphaned_downloads() -> Result<(), Error> {
    let has_in_progress = {
        let _lock = StateLock::acquire()?;
        has_in_progress(&read_state_unlocked()?.downloads)
    };
    if has_in_progress && !browser_launcher_alive() {
        interrupt_in_progress_downloads("browser_process_exited_before_completion")?;
    }
    Ok(())
}

pub fn download_records() -> Result<Vec<DownloadRecord>, Error> {
    reconcile_orphaned_downloads()?;
    let _lock = StateLock::acquire()?;
    let mut records = read_state_unlocked()?.downloads;
    records.sort_by_key(|record| std::cmp::Reverse(record.started_at_ms));
    Ok(records)
}

pub fn download_record(id: &str) -> Result<DownloadRecord, Error> {
    download_records()?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::DownloadFailed,
                format!("download not found: {id}"),
                false,
            )
        })
}

fn upsert_will_begin(params: &Value) -> Result<(), Error> {
    let id = params["guid"].as_str().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Browser.downloadWillBegin is missing guid",
            false,
        )
    })?;
    let url = params["url"].as_str().unwrap_or("");
    let suggested = params["suggestedFilename"].as_str().unwrap_or("download");
    let record = new_record(id, sanitize_url(url), suggested, now_ms());
    with_state_mut(|state| {
        if let Some(existing) = state.downloads.iter_mut().find(|record| record.id == id) {
            *existing = record;
        } else {
            state.downloads.push(record);
        }
        Ok(())
    })
}

fn number_u64(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|value| value.max(0.0) as u64))
        .unwrap_or(0)
}

fn update_progress(params: &Value) -> Result<(), Error> {
    let id = params["guid"].as_str().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Browser.downloadProgress is missing guid",
            false,
        )
    })?;
    let protocol_state = params["state"].as_str().unwrap_or("inProgress");
    let received = number_u64(&params["receivedBytes"]);
    let total = number_u64(&params["totalBytes"]);
    let event_path = params["filePath"].as_str();
    with_state_mut(|state| {
        let Some(record) = state.downloads.iter_mut().find(|record| record.id == id) else {
            return Ok(());
        };
        // Observation stays in the shell and happens only for a completed download
        // without a CDP-supplied file path (the previous fallback behavior).
        let fallback_path = if protocol_state == "completed" && event_path.is_none() {
            let candidate = Path::new(DOWNLOAD_DIR.as_str()).join(id);
            candidate.exists().then(|| candidate.display().to_string())
        } else {
            None
        };
        *record = apply_progress(
            record,
            &Progress {
                protocol_state,
                received_bytes: received,
                total_bytes: total,
                event_path,
                fallback_path: fallback_path.as_deref(),
                now_ms: now_ms(),
            },
        );
        Ok(())
    })
}

fn interrupt_in_progress_downloads(reason: &str) -> Result<(), Error> {
    with_state_mut(|state| {
        let now = now_ms();
        for record in &mut state.downloads {
            if record.state == "in_progress" {
                *record = interrupt_record(record, reason, now);
            }
        }
        Ok(())
    })
}

fn interrupt_stale_downloads() -> Result<(), Error> {
    interrupt_in_progress_downloads("browser_restarted_before_completion")
}

pub(crate) fn interrupt_downloads_on_browser_stop() -> Result<(), Error> {
    interrupt_in_progress_downloads("browser_stopped_before_completion")
}

fn recv_response<S: std::io::Read + std::io::Write>(
    ws: &mut WebSocket<S>,
    id: i64,
) -> Result<Value, Error> {
    loop {
        let message = ws.read().map_err(|error| {
            jelly_error(
                ErrorKind::BrowserUnavailable,
                format!("download tracker CDP read failed: {error}"),
                true,
            )
        })?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(&text)?;
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            if let Some(error) = value.get("error") {
                return Err(jelly_error(
                    ErrorKind::CdpFailed,
                    format!("Browser.setDownloadBehavior failed: {error}"),
                    false,
                ));
            }
            return Ok(value);
        }
    }
}

pub fn start_download_tracker(endpoint: &str) -> Result<thread::JoinHandle<()>, Error> {
    fs::create_dir_all(DOWNLOAD_DIR)?;
    interrupt_stale_downloads()?;
    let (mut ws, _) = connect(endpoint).map_err(|error| {
        jelly_error(
            ErrorKind::BrowserUnavailable,
            format!("download tracker could not connect to Chromium: {error}"),
            true,
        )
    })?;
    ws.send(Message::Text(
        json!({
            "id": 1,
            "method": "Browser.setDownloadBehavior",
            "params": {
                "behavior": "allowAndName",
                "downloadPath": DOWNLOAD_DIR.as_str(),
                "eventsEnabled": true
            }
        })
        .to_string()
        .into(),
    ))
    .map_err(|error| {
        jelly_error(
            ErrorKind::BrowserUnavailable,
            format!("download tracker could not configure Chromium: {error}"),
            true,
        )
    })?;
    recv_response(&mut ws, 1)?;

    Ok(thread::spawn(move || {
        loop {
            let message = match ws.read() {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("Jelly download tracker stopped: {error}");
                    if let Err(state_error) =
                        interrupt_in_progress_downloads("download_tracker_disconnected")
                    {
                        eprintln!(
                            "Jelly download tracker could not persist interruption state: {state_error}"
                        );
                    }
                    break;
                }
            };
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("Jelly download tracker ignored invalid CDP event: {error}");
                    continue;
                }
            };
            let result = match value["method"].as_str() {
                Some("Browser.downloadWillBegin") => upsert_will_begin(&value["params"]),
                Some("Browser.downloadProgress") => update_progress(&value["params"]),
                _ => Ok(()),
            };
            if let Err(error) = result {
                eprintln!("Jelly download tracker state update failed: {error}");
            }
        }
    }))
}

pub fn cancel_download(id: &str) -> Result<DownloadRecord, Error> {
    let record = download_record(id)?;
    if record.state != "in_progress" {
        return Err(jelly_error(
            ErrorKind::DownloadFailed,
            format!("download {id} is not in progress (state: {})", record.state),
            false,
        ));
    }
    let mut browser = BrowserSession::connect()?;
    browser.browser_call("Browser.cancelDownload", json!({"guid":id}))?;
    with_state_mut(|state| {
        let record = state
            .downloads
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::DownloadFailed,
                    format!("download not found: {id}"),
                    false,
                )
            })?;
        *record = request_cancel(record, now_ms());
        Ok(record.clone())
    })
}

fn copy_create_new(source: &Path, output: &Path) -> io::Result<u64> {
    let mut input = File::open(source)?;
    let mut target = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    match io::copy(&mut input, &mut target) {
        Ok(bytes) => {
            target.sync_all()?;
            Ok(bytes)
        }
        Err(error) => {
            drop(target);
            let _ = fs::remove_file(output);
            Err(error)
        }
    }
}

fn completed_source(record: &DownloadRecord) -> Result<PathBuf, Error> {
    completed_source_in(record, Path::new(DOWNLOAD_DIR.as_str()))
}

// Take the managed directory explicitly so a test can exercise file selection
// without inspecting the active Jelly download directory.
fn completed_source_in(record: &DownloadRecord, managed_dir: &Path) -> Result<PathBuf, Error> {
    let candidates = [
        record.file_path.as_deref().map(PathBuf::from),
        Some(managed_dir.join(&record.id)),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::DownloadFailed,
                format!(
                    "download {} completed but its managed file is not available",
                    record.id
                ),
                true,
            )
        })
}

fn materialize(
    record: &DownloadRecord,
    destination: Option<&str>,
    collision: &str,
) -> Result<(PathBuf, Option<String>), Error> {
    materialize_in(
        record,
        destination,
        collision,
        Path::new(DOWNLOAD_DIR.as_str()),
    )
}

fn materialize_in(
    record: &DownloadRecord,
    destination: Option<&str>,
    collision: &str,
    managed_dir: &Path,
) -> Result<(PathBuf, Option<String>), Error> {
    // Resolve source before policy validation to preserve the existing error order.
    let source = completed_source_in(record, managed_dir)?;
    materialize_source(
        &source,
        &record.suggested_filename,
        &record.id,
        destination,
        collision,
    )
}

/// Isolated file adapter; unlike `materialize`, this does not read global paths.
fn materialize_source(
    source: &Path,
    filename: &str,
    fallback_id: &str,
    destination: Option<&str>,
    collision: &str,
) -> Result<(PathBuf, Option<String>), Error> {
    let plan =
        plan_materialization(destination, collision, filename, fallback_id).map_err(|()| {
            jelly_error(
                ErrorKind::InvalidArguments,
                "download collision policy must be fail, overwrite, or uniquify",
                false,
            )
        })?;
    let (directory, requested, policy) = match plan {
        MaterializationPlan::UseManagedFile => return Ok((source.to_path_buf(), None)),
        MaterializationPlan::Copy {
            directory,
            requested,
            policy,
        } => (directory, requested, policy),
    };
    // The plan is pure; side effects still happen in the same order.
    fs::create_dir_all(&directory)?;
    let output = match policy {
        CollisionPolicy::Overwrite => {
            fs::copy(source, &requested)?;
            requested
        }
        CollisionPolicy::Fail => {
            match copy_create_new(source, &requested) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(jelly_error(
                        ErrorKind::DownloadFailed,
                        format!(
                            "download destination already exists: {}",
                            requested.display()
                        ),
                        false,
                    ));
                }
                Err(error) => return Err(error.into()),
            }
            requested
        }
        CollisionPolicy::Uniquify => {
            let mut copied = None;
            for index in 0..=10_000u32 {
                let candidate = numbered_path(&requested, index);
                match copy_create_new(source, &candidate) {
                    Ok(_) => {
                        copied = Some(candidate);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            copied.ok_or_else(|| {
                jelly_error(
                    ErrorKind::DownloadFailed,
                    format!(
                        "could not allocate a unique download destination for {}",
                        requested.display()
                    ),
                    true,
                )
            })?
        }
    };
    Ok((output, Some(policy.name().to_owned())))
}

pub fn finalize_download(
    id: &str,
    destination: Option<&str>,
    collision: &str,
) -> Result<DownloadRecord, Error> {
    let record = download_record(id)?;
    if let FinalizeState::NotCompleted { retryable } = finalize_state(&record) {
        return Err(jelly_error(
            ErrorKind::DownloadFailed,
            format!("download {id} is not completed (state: {})", record.state),
            retryable,
        ));
    }
    let (path, collision_policy) = materialize(&record, destination, collision)?;
    let artifact = register_download_with_context(
        &path,
        (!record.url.is_empty()).then_some(record.url.as_str()),
        None,
        Some(&record.id),
        Some(&record.suggested_filename),
    )?;
    with_state_mut(|state| {
        let record = state
            .downloads
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::DownloadFailed,
                    format!("download not found: {id}"),
                    false,
                )
            })?;
        // Keep this filesystem observation inside the lock, after artifact
        // registration; failure must still prevent the state write.
        let source = completed_source(record)?.display().to_string();
        *record = finalized_record(
            record,
            source,
            path.display().to_string(),
            collision_policy,
            artifact,
            now_ms(),
        );
        Ok(record.clone())
    })
}

pub fn wait_download(
    id: &str,
    seconds: u64,
    destination: Option<&str>,
    collision: &str,
) -> Result<DownloadRecord, Error> {
    let deadline = Instant::now() + Duration::from_secs(seconds.max(1));
    loop {
        let record = download_record(id)?;
        match wait_state(&record) {
            WaitState::Finalize => return finalize_download(id, destination, collision),
            WaitState::Failed => {
                return Err(jelly_error(
                    ErrorKind::DownloadFailed,
                    format!(
                        "download {id} ended in state {} ({})",
                        record.state,
                        record.failure_reason.as_deref().unwrap_or("unknown reason")
                    ),
                    false,
                ));
            }
            WaitState::Pending => {}
        }
        if Instant::now() >= deadline {
            return Err(jelly_error(
                ErrorKind::ConditionTimeout,
                format!("timed out after {seconds}s waiting for download {id}"),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::downloads::safe_filename;

    struct IsolatedDownloadDir {
        root: PathBuf,
    }

    impl IsolatedDownloadDir {
        fn new() -> Self {
            let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "jelly-isolated-download-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("unique isolated test root");
            Self { root }
        }

        fn destination(&self, name: &str) -> String {
            self.root.join(name).to_string_lossy().into_owned()
        }

        fn source(&self) -> PathBuf {
            let source = self.root.join("managed-source");
            fs::write(&source, b"new-content").unwrap();
            source
        }
    }

    impl Drop for IsolatedDownloadDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn completed_source_prefers_reported_path_and_then_falls_back_to_managed_guid() {
        let isolated = IsolatedDownloadDir::new();
        let managed = isolated.root.join("managed");
        fs::create_dir(&managed).unwrap();
        let mut record = new_record("guid-a", "".into(), "report.pdf", 1);
        let missing = isolated.root.join("no-such-file");
        record.file_path = Some(missing.display().to_string());
        let error = completed_source_in(&record, &managed).unwrap_err();
        assert!(error.to_string().contains("managed file is not available"));
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::DownloadFailed, true)
        );

        let fallback = managed.join("guid-a");
        fs::write(&fallback, b"fallback").unwrap();
        assert_eq!(completed_source_in(&record, &managed).unwrap(), fallback);
        let reported = isolated.root.join("preferred");
        fs::write(&reported, b"preferred").unwrap();
        record.file_path = Some(reported.display().to_string());
        assert_eq!(completed_source_in(&record, &managed).unwrap(), reported);
    }

    #[test]
    fn source_error_precedes_bad_policy_and_cannot_create_destination() {
        let isolated = IsolatedDownloadDir::new();
        let managed = isolated.root.join("managed");
        fs::create_dir(&managed).unwrap();
        let record = new_record("guid-a", "".into(), "report.pdf", 1);
        let output = isolated.destination("should-not-exist");
        let error = materialize_in(&record, Some(&output), "invalid", &managed).unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::DownloadFailed, true)
        );
        assert!(error.to_string().contains("managed file is not available"));
        assert!(!Path::new(&output).exists());
        // Once a valid source is observed, the same request fails on its policy.
        fs::write(managed.join("guid-a"), b"managed-content").unwrap();
        let error = materialize_in(&record, Some(&output), "invalid", &managed).unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
        assert!(!Path::new(&output).exists());
    }

    #[test]
    fn materialization_uses_managed_fallback_without_using_global_download_root() {
        let isolated = IsolatedDownloadDir::new();
        let managed = isolated.root.join("managed");
        fs::create_dir(&managed).unwrap();
        fs::write(managed.join("guid-a"), b"managed-content").unwrap();
        let mut record = new_record("guid-a", "".into(), "report.csv", 1);
        record.file_path = Some(isolated.root.join("stale-path").display().to_string());
        let output_dir = isolated.destination("out");
        let (path, policy) = materialize_in(&record, Some(&output_dir), "fail", &managed).unwrap();
        assert_eq!(path, Path::new(&output_dir).join("report.csv"));
        assert_eq!(policy.as_deref(), Some("fail"));
        assert_eq!(fs::read(path).unwrap(), b"managed-content");
        assert_eq!(
            fs::read(managed.join("guid-a")).unwrap(),
            b"managed-content"
        );
    }

    #[test]
    fn no_destination_returns_source_without_validating_collision_or_copying() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.source();
        let (output, policy) =
            materialize_source(&source, "report.pdf", "guid-a", None, "unsupported").unwrap();
        assert_eq!(output, source);
        assert_eq!(policy, None);
        assert_eq!(fs::read(output).unwrap(), b"new-content");
        assert_eq!(fs::read_dir(&isolated.root).unwrap().count(), 1);
    }

    #[test]
    fn invalid_policy_fails_without_creating_any_destination_directory() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.source();
        let directory = isolated.destination("never-created");
        let error = materialize_source(
            &source,
            "report.pdf",
            "guid-a",
            Some(&directory),
            "unsupported",
        )
        .unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
        assert_eq!(
            error.to_string(),
            "download collision policy must be fail, overwrite, or uniquify"
        );
        assert!(!Path::new(&directory).exists());
    }

    #[test]
    fn fail_policy_creates_a_new_file_and_never_overwrites_an_existing_file() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.source();
        let directory = isolated.destination("fail-policy");
        let (output, policy) =
            materialize_source(&source, "../report.pdf", "guid-a", Some(&directory), "fail")
                .unwrap();
        assert_eq!(output, Path::new(&directory).join("report.pdf"));
        assert_eq!(policy.as_deref(), Some("fail"));
        assert_eq!(fs::read(&output).unwrap(), b"new-content");
        fs::write(&output, b"existing-content").unwrap();
        let error = materialize_source(&source, "report.pdf", "guid-a", Some(&directory), "fail")
            .unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::DownloadFailed, false)
        );
        assert_eq!(
            error.to_string(),
            format!("download destination already exists: {}", output.display())
        );
        assert_eq!(fs::read(output).unwrap(), b"existing-content");
        assert_eq!(fs::read(source).unwrap(), b"new-content");
    }

    #[test]
    fn overwrite_policy_replaces_an_existing_file_without_renaming_it() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.source();
        let directory = isolated.destination("overwrite-policy");
        fs::create_dir(&directory).unwrap();
        let requested = Path::new(&directory).join("report.pdf");
        fs::write(&requested, b"existing-content").unwrap();
        let (output, policy) = materialize_source(
            &source,
            "report.pdf",
            "guid-a",
            Some(&directory),
            "overwrite",
        )
        .unwrap();
        assert_eq!(output, requested);
        assert_eq!(policy.as_deref(), Some("overwrite"));
        assert_eq!(fs::read(output).unwrap(), b"new-content");
    }

    #[test]
    fn uniquify_policy_skips_existing_names_and_preserves_the_original() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.source();
        let directory = isolated.destination("uniquify-policy");
        fs::create_dir(&directory).unwrap();
        let requested = Path::new(&directory).join("report.pdf");
        let occupied = Path::new(&directory).join("report (1).pdf");
        fs::write(&requested, b"existing-0").unwrap();
        fs::write(&occupied, b"existing-1").unwrap();
        let (output, policy) = materialize_source(
            &source,
            "report.pdf",
            "guid-a",
            Some(&directory),
            "uniquify",
        )
        .unwrap();
        assert_eq!(output, Path::new(&directory).join("report (2).pdf"));
        assert_eq!(policy.as_deref(), Some("uniquify"));
        assert_eq!(fs::read(output).unwrap(), b"new-content");
        assert_eq!(fs::read(requested).unwrap(), b"existing-0");
        assert_eq!(fs::read(occupied).unwrap(), b"existing-1");
    }

    #[test]
    fn materialization_missing_source_preserves_read_error_and_does_not_create_file() {
        let isolated = IsolatedDownloadDir::new();
        let source = isolated.root.join("missing-source");
        let directory = isolated.destination("new-directory");
        let error = materialize_source(&source, "report.pdf", "guid-a", Some(&directory), "fail")
            .unwrap_err();
        assert!(
            error.to_string().contains("No such file") || error.to_string().contains("not found")
        );
        // The shell had already created the directory before opening the input.
        assert!(Path::new(&directory).is_dir());
        assert!(!Path::new(&directory).join("report.pdf").exists());
    }

    #[test]
    fn suggested_filename_is_reduced_to_a_safe_basename() {
        assert_eq!(safe_filename("../report.csv", "fallback"), "report.csv");
        assert_eq!(safe_filename("", "fallback"), "fallback");
    }

    #[test]
    fn numbered_destination_preserves_extension() {
        let path = PathBuf::from("/tmp/report.csv");
        assert_eq!(
            numbered_path(&path, 1)
                .file_name()
                .unwrap()
                .to_string_lossy(),
            "report (1).csv"
        );
        assert_eq!(numbered_path(&path, 0), path);
    }

    #[test]
    fn exclusive_copy_never_overwrites_an_existing_destination() {
        let root = std::env::temp_dir().join(format!("jelly-download-test-{}", now_ms()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let output = root.join("output");
        fs::write(&source, b"new").unwrap();
        fs::write(&output, b"existing").unwrap();
        assert_eq!(
            copy_create_new(&source, &output).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&output).unwrap(), b"existing");
        let _ = fs::remove_dir_all(root);
    }
}

// This test is deliberately ignored in ordinary Cargo runs. It may mutate only
// a configuration *compiled from an independent FC/IS sandbox checkout* and
// requires a matching explicit opt-in. Do not relax either guard.
#[cfg(test)]
mod fcis_isolated_integration {
    use super::*;

    #[test]
    #[ignore = "requires isolated copied repo configuration and JELLY_FCIS_ISOLATION_ROOT"]
    fn fcis_real_download_persistence_and_artifact_finalize() {
        let requested = std::env::var("JELLY_FCIS_ISOLATION_ROOT")
            .expect("set explicit sandbox root before running this ignored integration test");
        let sandbox = PathBuf::from(&requested)
            .canonicalize()
            .expect("sandbox must exist");
        assert_eq!(sandbox.parent(), Some(Path::new("/data")));
        assert!(
            sandbox
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("jelly-fcis-isolated-"))
        );
        assert_eq!(
            crate::config::runtime_paths().root,
            sandbox.join("runtime"),
            "refusing to write outside isolated runtime"
        );
        assert!(!sandbox.join("repo/.env").exists());

        let id = format!(
            "fcis-{}-{}",
            std::process::id(),
            TEMP_NONCE.fetch_add(1, Ordering::Relaxed)
        );
        let source = Path::new(DOWNLOAD_DIR.as_str()).join(&id);
        let output_dir = sandbox.join("download-test-output").join(&id);
        fs::create_dir_all(DOWNLOAD_DIR.as_str()).unwrap();

        upsert_will_begin(&json!({
            "guid":id,
            "url":"https://example.test/report.txt",
            "suggestedFilename":"report.txt"
        }))
        .unwrap();
        let new = read_state_unlocked().unwrap();
        assert_eq!(
            new.downloads
                .iter()
                .find(|record| record.id == id)
                .unwrap()
                .state,
            "in_progress"
        );

        fs::write(&source, b"isolated-completed-content").unwrap();
        update_progress(&json!({
            "guid":id,
            "state":"completed",
            "receivedBytes":26,
            "totalBytes":26,
            "filePath":source.to_string_lossy()
        }))
        .unwrap();
        let completed = download_record(&id).unwrap();
        assert_eq!(completed.state, "completed");
        assert!(completed.artifact.is_none());

        let destination = output_dir.to_string_lossy();
        let first = wait_download(&id, 1, Some(&destination), "fail").unwrap();
        let first_path = output_dir.join("report.txt");
        assert_eq!(first.materialized_path.as_deref(), first_path.to_str());
        assert_eq!(first.collision_policy.as_deref(), Some("fail"));
        assert_eq!(
            fs::read(&first_path).unwrap(),
            b"isolated-completed-content"
        );
        let artifact = first.artifact.as_ref().unwrap();
        assert_eq!(artifact["kind"], "download");
        assert_eq!(artifact["source"]["download_id"], id);
        assert_eq!(artifact["properties"]["bytes"], 26);
        assert!(Path::new(artifact["path"].as_str().unwrap()).is_file());
        let artifact_id = artifact["artifact_id"].as_str().unwrap();
        assert!(
            Path::new(crate::ARTIFACT_META_DIR.as_str())
                .join(format!("{artifact_id}.json"))
                .is_file()
        );
        let persisted = read_state_unlocked().unwrap();
        assert_eq!(persisted.version, DOWNLOAD_STATE_VERSION);
        assert_eq!(
            persisted
                .downloads
                .iter()
                .find(|record| record.id == id)
                .unwrap()
                .artifact,
            Some(artifact.clone())
        );

        let conflict = finalize_download(&id, Some(&destination), "fail").unwrap_err();
        assert_eq!(
            crate::classify_error(conflict.as_ref()),
            (ErrorKind::DownloadFailed, false)
        );
        let unique = finalize_download(&id, Some(&destination), "uniquify").unwrap();
        let unique_path = output_dir.join("report (1).txt");
        assert_eq!(unique.materialized_path.as_deref(), unique_path.to_str());
        assert_eq!(
            fs::read(&unique_path).unwrap(),
            b"isolated-completed-content"
        );
        assert_eq!(
            fs::read(&first_path).unwrap(),
            b"isolated-completed-content"
        );
        assert_eq!(unique.collision_policy.as_deref(), Some("uniquify"));

        let stale_id = format!("{id}-stale");
        upsert_will_begin(&json!({
            "guid":stale_id, "url":"https://example.test/stale", "suggestedFilename":"stale.txt"
        }))
        .unwrap();
        interrupt_stale_downloads().unwrap();
        let stale = download_record(&stale_id).unwrap();
        assert_eq!(stale.state, "interrupted");
        assert_eq!(
            stale.failure_reason.as_deref(),
            Some("browser_restarted_before_completion")
        );
        assert_eq!(download_record(&id).unwrap().state, "completed");
    }
}
