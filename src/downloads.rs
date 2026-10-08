use crate::{
    BROWSER_PID, BrowserSession, DOWNLOAD_DIR, Error, ErrorKind, STATE_DIR, jelly_error,
    register_download_with_context, sanitize_url,
};
use serde::{Deserialize, Serialize};
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

const DOWNLOAD_STATE_VERSION: u32 = 1;
const MAX_DOWNLOAD_RECORDS: usize = 512;
static TEMP_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub suggested_filename: String,
    pub state: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub materialized_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadState {
    version: u32,
    downloads: Vec<DownloadRecord>,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            version: DOWNLOAD_STATE_VERSION,
            downloads: Vec::new(),
        }
    }
}

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
        read_state_unlocked()?
            .downloads
            .iter()
            .any(|record| record.state == "in_progress")
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
    let now = now_ms();
    with_state_mut(|state| {
        let record = DownloadRecord {
            id: id.to_owned(),
            url: sanitize_url(url),
            suggested_filename: suggested.to_owned(),
            state: "in_progress".into(),
            received_bytes: 0,
            total_bytes: 0,
            started_at_ms: now,
            updated_at_ms: now,
            file_path: None,
            materialized_path: None,
            collision_policy: None,
            failure_reason: None,
            cancel_requested: false,
            artifact: None,
        };
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
    let event_path = params["filePath"].as_str().map(str::to_owned);
    with_state_mut(|state| {
        let Some(record) = state.downloads.iter_mut().find(|record| record.id == id) else {
            return Ok(());
        };
        record.received_bytes = received;
        record.total_bytes = total;
        record.updated_at_ms = now_ms();
        match protocol_state {
            "completed" => {
                record.state = "completed".into();
                record.failure_reason = None;
                record.file_path = event_path.clone().or_else(|| {
                    let candidate = Path::new(DOWNLOAD_DIR.as_str()).join(id);
                    candidate.exists().then(|| candidate.display().to_string())
                });
            }
            "canceled" => {
                record.state = "canceled".into();
                record.failure_reason = Some(
                    if record.cancel_requested {
                        "canceled_by_user"
                    } else {
                        "canceled_by_browser"
                    }
                    .into(),
                );
            }
            _ => {
                record.state = "in_progress".into();
            }
        }
        Ok(())
    })
}

fn interrupt_in_progress_downloads(reason: &str) -> Result<(), Error> {
    with_state_mut(|state| {
        let now = now_ms();
        for record in &mut state.downloads {
            if record.state == "in_progress" {
                record.state = "interrupted".into();
                record.updated_at_ms = now;
                record.failure_reason = Some(reason.to_owned());
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
        record.cancel_requested = true;
        record.updated_at_ms = now_ms();
        if record.state == "canceled" {
            record.failure_reason = Some("canceled_by_user".into());
        }
        Ok(record.clone())
    })
}

fn safe_filename(value: &str, fallback: &str) -> String {
    Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .unwrap_or(fallback)
        .to_owned()
}

fn numbered_path(path: &Path, index: u32) -> PathBuf {
    if index == 0 {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    let name = match extension {
        Some(extension) => format!("{stem} ({index}).{extension}"),
        None => format!("{stem} ({index})"),
    };
    parent.join(name)
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
    let candidates = [
        record.file_path.as_deref().map(PathBuf::from),
        Some(Path::new(DOWNLOAD_DIR.as_str()).join(&record.id)),
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
    let source = completed_source(record)?;
    let Some(destination) = destination else {
        return Ok((source, None));
    };
    if !matches!(collision, "fail" | "overwrite" | "uniquify") {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "download collision policy must be fail, overwrite, or uniquify",
            false,
        ));
    }
    let directory = PathBuf::from(destination);
    fs::create_dir_all(&directory)?;
    let filename = safe_filename(&record.suggested_filename, &record.id);
    let requested = directory.join(filename);
    let output = match collision {
        "overwrite" => {
            fs::copy(&source, &requested)?;
            requested
        }
        "fail" => {
            match copy_create_new(&source, &requested) {
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
        "uniquify" => {
            let mut copied = None;
            for index in 0..=10_000u32 {
                let candidate = numbered_path(&requested, index);
                match copy_create_new(&source, &candidate) {
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
        _ => unreachable!(),
    };
    Ok((output, Some(collision.to_owned())))
}

pub fn finalize_download(
    id: &str,
    destination: Option<&str>,
    collision: &str,
) -> Result<DownloadRecord, Error> {
    let record = download_record(id)?;
    if record.state != "completed" {
        return Err(jelly_error(
            ErrorKind::DownloadFailed,
            format!("download {id} is not completed (state: {})", record.state),
            record.state == "in_progress",
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
        record.file_path = Some(completed_source(record)?.display().to_string());
        record.materialized_path = Some(path.display().to_string());
        record.collision_policy = collision_policy;
        record.artifact = Some(artifact);
        record.updated_at_ms = now_ms();
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
        match record.state.as_str() {
            "completed" => return finalize_download(id, destination, collision),
            "canceled" | "interrupted" => {
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
            _ => {}
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
