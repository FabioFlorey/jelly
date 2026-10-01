use crate::{
    ARTIFACT_META_DIR, DOWNLOAD_DIR, Error, ErrorKind, RECORDING_DIR, SCREENSHOT_DIR, jelly_error,
    new_id,
};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

fn png_dimensions(path: &Path) -> Result<(u32, u32), Error> {
    let mut file = fs::File::open(path)?;
    let mut header = [0_u8; 24];
    file.read_exact(&mut header)?;
    if &header[0..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("artifact is not a valid PNG: {}", path.display()),
            false,
        ));
    }
    let width = u32::from_be_bytes(header[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(header[20..24].try_into().unwrap());
    if width == 0 || height == 0 {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("PNG has zero dimensions: {}", path.display()),
            false,
        ));
    }
    Ok((width, height))
}

fn absolute(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn sanitize_url(value: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(value) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string();
    }
    value.split(['?', '#']).next().unwrap_or(value).to_owned()
}

fn safe_source_url(value: Option<&str>) -> Option<String> {
    value.map(sanitize_url)
}

fn valid_artifact_id(id: &str) -> bool {
    let mut parts = id.split('-');
    matches!(parts.next(), Some("artifact"))
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && parts.next().is_none()
}

fn metadata_path(id: &str) -> Result<PathBuf, Error> {
    if !valid_artifact_id(id) {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("invalid artifact id: {id}"),
            false,
        ));
    }
    Ok(Path::new(ARTIFACT_META_DIR).join(format!("{id}.json")))
}

#[cfg(unix)]
fn private_file(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn private_file(_: &Path) -> Result<(), Error> {
    Ok(())
}

fn write_metadata_path(path: &Path, value: &Value) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", new_id("write")));
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    private_file(&tmp)?;
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}

fn write_metadata(id: &str, value: &Value) -> Result<(), Error> {
    let path = metadata_path(id)?;
    write_metadata_path(&path, value)
}

pub fn register_screenshot(
    path: impl AsRef<Path>,
    target: Option<&str>,
    source_url: Option<&str>,
    source_title: Option<&str>,
) -> Result<Value, Error> {
    let source_path = absolute(path.as_ref());
    private_file(&source_path)?;
    let metadata = fs::metadata(&source_path)?;
    if metadata.len() == 0 {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("screenshot is empty: {}", source_path.display()),
            false,
        ));
    }
    let (width, height) = png_dimensions(&source_path)?;
    let id = new_id("artifact");
    let archive_dir = Path::new(SCREENSHOT_DIR).join("registered");
    fs::create_dir_all(&archive_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive_dir, fs::Permissions::from_mode(0o700))?;
    }
    let archived = archive_dir.join(format!("{id}.png"));
    fs::copy(&source_path, &archived)?;
    private_file(&archived)?;
    let path = absolute(&archived);
    let created_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let trace_id = env::var("JELLY_TRACE_ID").ok();
    let value = json!({
        "artifact_id": id,
        "kind": "screenshot",
        "path": path,
        "created_at": created_at,
        "source": {
            "url": safe_source_url(source_url),
            "title": source_title
        },
        "target": target,
        "properties": {
            "mime": "image/png",
            "bytes": metadata.len(),
            "width": width,
            "height": height
        },
        "verification": {
            "integrity_verified": true,
            "semantic_verified": false,
            "checks": ["file_exists", "nonzero_bytes", "png_dimensions"]
        },
        "trace_id": trace_id
    });
    write_metadata(value["artifact_id"].as_str().unwrap(), &value)?;
    Ok(value)
}

fn resolve_artifact(value: &str) -> Result<(PathBuf, Option<PathBuf>, Value), Error> {
    if valid_artifact_id(value) {
        let by_id = metadata_path(value)?;
        if !by_id.is_file() {
            return Err(jelly_error(
                ErrorKind::ArtifactFailed,
                format!("artifact not found: {value}"),
                false,
            ));
        }
        let metadata: Value = serde_json::from_slice(&fs::read(&by_id)?)?;
        let path = metadata["path"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::ArtifactFailed,
                    "artifact metadata has no path",
                    false,
                )
            })?;
        return Ok((path, Some(by_id), metadata));
    }

    let path = absolute(Path::new(value));
    if !path.is_file() {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("artifact not found: {value}"),
            false,
        ));
    }

    if let Ok(entries) = fs::read_dir(ARTIFACT_META_DIR) {
        for entry in entries.flatten() {
            let meta_path = entry.path();
            if meta_path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&meta_path) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let Some(recorded) = metadata["path"].as_str() else {
                continue;
            };
            if absolute(Path::new(recorded)) == path {
                return Ok((path, Some(meta_path), metadata));
            }
        }
    }

    Ok((
        path.clone(),
        None,
        json!({
            "artifact_id": Value::Null,
            "kind": "unknown",
            "path": path,
            "verification": {}
        }),
    ))
}

pub fn register_recording(
    video_path: impl AsRef<Path>,
    manifest_path: impl AsRef<Path>,
    source_url: Option<&str>,
    source_title: Option<&str>,
    target_id: Option<&str>,
    frame_count: u64,
) -> Result<Value, Error> {
    let source_video = absolute(video_path.as_ref());
    let source_manifest = absolute(manifest_path.as_ref());
    private_file(&source_video)?;
    private_file(&source_manifest)?;
    let video_metadata = fs::metadata(&source_video)?;
    let manifest_metadata = fs::metadata(&source_manifest)?;
    if video_metadata.len() == 0 || manifest_metadata.len() == 0 || frame_count == 0 {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            "browser recording is incomplete",
            false,
        ));
    }
    let id = new_id("artifact");
    let archive_dir = Path::new(RECORDING_DIR).join("registered");
    fs::create_dir_all(&archive_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive_dir, fs::Permissions::from_mode(0o700))?;
    }
    let archived_video = archive_dir.join(format!("{id}.mp4"));
    let archived_manifest = archive_dir.join(format!("{id}.json"));
    fs::copy(&source_video, &archived_video)?;
    fs::copy(&source_manifest, &archived_manifest)?;
    private_file(&archived_video)?;
    private_file(&archived_manifest)?;
    let path = absolute(&archived_video);
    let manifest_path = absolute(&archived_manifest);
    let created_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let trace_id = env::var("JELLY_TRACE_ID").ok();
    let value = json!({
        "artifact_id": id,
        "kind": "browser_recording",
        "path": path,
        "manifest_path": manifest_path,
        "created_at": created_at,
        "source": {
            "url": safe_source_url(source_url),
            "title": source_title,
            "target_id": target_id
        },
        "properties": {
            "mime": "video/mp4",
            "bytes": video_metadata.len(),
            "manifest_bytes": manifest_metadata.len(),
            "frame_count": frame_count
        },
        "verification": {
            "integrity_verified": true,
            "semantic_verified": false,
            "checks": ["file_exists", "nonzero_bytes", "manifest_exists", "frame_count"]
        },
        "trace_id": trace_id
    });
    write_metadata(value["artifact_id"].as_str().unwrap(), &value)?;
    Ok(value)
}

pub fn register_download(
    path: impl AsRef<Path>,
    source_url: Option<&str>,
    source_title: Option<&str>,
) -> Result<Value, Error> {
    let source_path = absolute(path.as_ref());
    let metadata = fs::metadata(&source_path)?;
    if metadata.len() == 0 {
        return Err(jelly_error(
            ErrorKind::DownloadFailed,
            format!("download is empty: {}", source_path.display()),
            false,
        ));
    }
    let id = new_id("artifact");
    let archive_dir = Path::new(DOWNLOAD_DIR).join("registered");
    fs::create_dir_all(&archive_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive_dir, fs::Permissions::from_mode(0o700))?;
    }
    let filename = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let path = archive_dir.join(format!("{id}-{filename}"));
    fs::copy(&source_path, &path)?;
    private_file(&path)?;
    let path = absolute(&path);
    let created_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
    let trace_id = env::var("JELLY_TRACE_ID").ok();
    let value = json!({
        "artifact_id": id,
        "kind": "download",
        "path": path,
        "created_at": created_at,
        "source": {"url": safe_source_url(source_url), "title": source_title},
        "properties": {"bytes": metadata.len()},
        "verification": {
            "integrity_verified": true,
            "semantic_verified": false,
            "checks": ["file_exists", "nonzero_bytes", "download_complete"]
        },
        "trace_id": trace_id
    });
    write_metadata(value["artifact_id"].as_str().unwrap(), &value)?;
    Ok(value)
}

pub fn verify_artifact(value: &str) -> Result<Value, Error> {
    let (path, metadata_path, mut record) = resolve_artifact(value)?;
    let file = fs::metadata(&path)?;
    if file.len() == 0 {
        return Err(jelly_error(
            ErrorKind::ArtifactFailed,
            format!("artifact is empty: {}", path.display()),
            false,
        ));
    }

    let mut checks = record["verification"]["checks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for check in ["file_exists", "nonzero_bytes"] {
        if !checks.iter().any(|value| value.as_str() == Some(check)) {
            checks.push(Value::String(check.into()));
        }
    }
    if path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("png"))
    {
        let (width, height) = png_dimensions(&path)?;
        if !checks
            .iter()
            .any(|value| value.as_str() == Some("png_dimensions"))
        {
            checks.push(Value::String("png_dimensions".into()));
        }
        record["properties"]["width"] = json!(width);
        record["properties"]["height"] = json!(height);
    }
    record["properties"]["bytes"] = json!(file.len());
    record["verification"]["integrity_verified"] = json!(true);
    record["verification"]["checks"] = Value::Array(checks);

    if let Some(path) = metadata_path {
        write_metadata_path(&path, &record)?;
    }
    Ok(record)
}

pub fn mark_artifact_verified(value: &str, checks: &[String]) -> Result<Value, Error> {
    let (_, metadata_path, mut record) = resolve_artifact(value)?;
    let metadata_path = metadata_path.ok_or_else(|| {
        jelly_error(
            ErrorKind::ArtifactFailed,
            "semantic verification requires a registered Jelly artifact",
            false,
        )
    })?;
    let mut combined = record["verification"]["checks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for check in checks {
        if !combined.iter().any(|value| value.as_str() == Some(check)) {
            combined.push(Value::String(check.clone()));
        }
    }
    record["verification"]["semantic_verified"] = json!(true);
    record["verification"]["produced_after_verification"] = json!(true);
    if record["kind"] == "screenshot" {
        record["verification"]["captured_after_verification"] = json!(true);
    }
    record["verification"]["checks"] = Value::Array(combined);
    write_metadata_path(&metadata_path, &record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dimension_parser_rejects_non_png() {
        let path =
            std::env::temp_dir().join(format!("jelly-artifact-test-{}.txt", std::process::id()));
        fs::write(&path, b"not png").unwrap();
        assert!(png_dimensions(&path).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn png_dimension_parser_reads_ihdr_dimensions() {
        let path =
            std::env::temp_dir().join(format!("jelly-artifact-test-{}.png", std::process::id()));
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&640_u32.to_be_bytes());
        bytes.extend_from_slice(&480_u32.to_be_bytes());
        fs::write(&path, bytes).unwrap();
        assert_eq!(png_dimensions(&path).unwrap(), (640, 480));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn metadata_updates_replace_valid_json_without_leaving_temp_files() {
        let dir = std::env::temp_dir().join(format!(
            "jelly-artifact-metadata-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("artifact.json");

        write_metadata_path(&path, &json!({"version": 1})).unwrap();
        write_metadata_path(&path, &json!({"version": 2})).unwrap();
        let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["version"], 2);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn artifact_ids_are_strict_and_cannot_escape_metadata_directory() {
        assert!(valid_artifact_id("artifact-123-456-7"));
        assert!(!valid_artifact_id("../../state/secret"));
        assert!(!valid_artifact_id("artifact-123-456-7/../../secret"));
        assert!(metadata_path("../../state/secret").is_err());
    }

    #[test]
    fn provenance_urls_drop_credentials_query_and_fragment() {
        assert_eq!(
            safe_source_url(Some(
                "https://user:pass@example.com/art/1?token=secret#view"
            )),
            Some("https://example.com/art/1".to_owned())
        );
    }
}
