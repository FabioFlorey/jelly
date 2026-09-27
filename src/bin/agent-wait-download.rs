use jelly::{
    BrowserSession, DOWNLOAD_DIR, ErrorKind, HEADLESS_PROFILE_DIR, PROFILE_DIR, jelly_error,
    register_download,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for directory in [
        PathBuf::from(DOWNLOAD_DIR),
        PathBuf::from(PROFILE_DIR).join("Downloads"),
        PathBuf::from(HEADLESS_PROFILE_DIR).join("Downloads"),
    ] {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_file()
                && !name.starts_with('.')
                && !name.ends_with(".crdownload")
                && !name.ends_with(".tmp")
                && !name.ends_with(".part")
                && !name.ends_with(".download")
            {
                out.push(path);
            }
        }
    }
    out
}

fn modified_ms(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn source_context() -> (Option<String>, Option<String>) {
    let Ok(mut browser) = BrowserSession::connect() else {
        return (None, None);
    };
    let Ok(value) = browser.eval("({url:location.href,title:document.title||''})") else {
        return (None, None);
    };
    (
        value["url"].as_str().map(str::to_owned),
        value["title"].as_str().map(str::to_owned),
    )
}

fn main() -> Result<(), jelly::Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    let after_ms = args
        .first()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::InvalidArguments,
                "usage: wait-download <after-ms> [seconds] [name-contains]",
                false,
            )
        })?
        .parse::<u128>()
        .map_err(|_| {
            jelly_error(
                ErrorKind::InvalidArguments,
                "after-ms must be an integer",
                false,
            )
        })?;
    let seconds = args
        .get(1)
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| {
            jelly_error(
                ErrorKind::InvalidArguments,
                "seconds must be an integer",
                false,
            )
        })?
        .unwrap_or(30);
    let name_contains = args.get(2).map(String::as_str);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let threshold = after_ms;

    loop {
        let mut matches = candidates()
            .into_iter()
            .filter(|path| modified_ms(path) >= threshold)
            .filter(|path| {
                name_contains.is_none_or(|needle| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains(needle))
                })
            })
            .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
            .collect::<Vec<_>>();
        matches.sort_by_key(|path| std::cmp::Reverse(modified_ms(path)));

        if let Some(path) = matches.first() {
            let first_size = fs::metadata(path)?.len();
            thread::sleep(Duration::from_millis(200));
            let stable = fs::metadata(path)
                .is_ok_and(|metadata| metadata.len() == first_size && first_size > 0);
            if stable {
                let (url, title) = source_context();
                let artifact = register_download(path, url.as_deref(), title.as_deref())?;
                println!("{}", serde_json::to_string(&artifact)?);
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(jelly_error(
                ErrorKind::ConditionTimeout,
                format!("timed out after {seconds}s waiting for a completed download"),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}
