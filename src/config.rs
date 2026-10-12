use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

#[derive(Debug, Clone, Deserialize)]
pub struct JellyConfig {
    pub paths: PathsConfig,
    pub browser: BrowserConfig,
    pub mcp: McpConfig,
    pub page: PageConfig,
    pub diagnostics: DiagnosticsConfig,
    pub tests: TestsConfig,
    pub ui: UiConfig,
    pub hitl: HitlConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathsConfig {
    pub runtime_root: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrowserConfig {
    #[serde(default)]
    pub backend: crate::browser_backend::BrowserBackendKind,
    /// Named remote CDP endpoints; select one via JELLY_BROWSER_ID per process.
    #[serde(default)]
    pub remotes: Vec<RemoteBrowserConfig>,
    pub startup_timeout_secs: u64,
    pub cdp_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteBrowserConfig {
    /// Stable UUID used for session and runtime identity.
    pub id: String,
    /// Optional human-readable selector; may change without migrating runtime state.
    #[serde(default)]
    pub name: Option<String>,
    pub endpoint: String,
    /// Transport topology metadata. This does not change CDP permissions or authentication.
    #[serde(default)]
    pub connection_mode: RemoteConnectionMode,
    /// Directory in the remote Chromium container where CDP writes GUID-named files.
    #[serde(default)]
    pub download_path: Option<String>,
    /// Operator-provided HTTPS endpoint serving GET /<GUID> with raw file bytes.
    #[serde(default)]
    pub download_base_url: Option<String>,
    /// Optional bearer token for the remote artifact endpoint.
    #[serde(default)]
    pub download_token: Option<String>,
    /// remote-only (default), shared-volume, or transfer.
    #[serde(default)]
    pub download_mode: RemoteDownloadMode,
    /// Host-mounted read-only directory for shared-volume mode.
    #[serde(default)]
    pub download_host_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteConnectionMode {
    #[default]
    Address,
    Subnet,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteDownloadMode {
    #[default]
    RemoteOnly,
    SharedVolume,
    Transfer,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    pub surface: String,
    pub persistent_session: bool,
    pub raw_cdp: bool,
    pub allow_cargo_fallback: bool,
    /// Optional client onboarding profiles. Empty preserves legacy MCP behavior.
    #[serde(default)]
    pub connections: Vec<crate::connection::ProviderConnection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageConfig {
    pub runtime: bool,
    pub snapshot_limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiagnosticsConfig {
    pub perf_log: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestsConfig {
    pub default_batch: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    pub animation: bool,
    pub icons: bool,
    pub logo: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HitlConfig {
    pub formats: HitlFormatsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HitlFormatsConfig {
    pub telegram: TelegramHitlFormatConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramHitlFormatConfig {
    pub title: String,
    pub subtitle: String,
    pub prefix: String,
    pub suffix: String,
    pub signature: String,
}

#[derive(Debug)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub state: PathBuf,
    pub profile: PathBuf,
    pub headless_profile: PathBuf,
    pub artifact_metadata: PathBuf,
    pub screenshots: PathBuf,
    pub recordings: PathBuf,
    pub downloads: PathBuf,
    pub network: PathBuf,
    pub logs: PathBuf,
    pub routines: PathBuf,
    pub injections: PathBuf,
    pub endpoint: PathBuf,
    pub active_target: PathBuf,
    pub logical_targets: PathBuf,
    pub logical_targets_lock: PathBuf,
    pub page_target: PathBuf,
    pub browser_pid: PathBuf,
    pub browser_stop: PathBuf,
    pub browser_mode: PathBuf,
    pub browser_ready: PathBuf,
}

impl RuntimePaths {
    fn from_root(root: PathBuf) -> Self {
        let state = root.join("state");
        let artifacts = root.join("artifacts");
        Self {
            profile: root.join("profiles/headed"),
            headless_profile: root.join("profiles/headless"),
            artifact_metadata: artifacts.join("metadata"),
            screenshots: artifacts.join("screenshots"),
            recordings: artifacts.join("recordings"),
            downloads: artifacts.join("downloads"),
            network: root.join("network"),
            logs: root.join("logs"),
            routines: root.join("routines"),
            injections: root.join("injections"),
            endpoint: state.join("cdp_endpoint"),
            active_target: state.join("active_target_id"),
            logical_targets: state.join("logical_targets.json"),
            logical_targets_lock: state.join("logical_targets.lock"),
            page_target: state.join("page_target_id"),
            browser_pid: state.join("browser.pid"),
            browser_stop: state.join("browser.stop"),
            browser_mode: state.join("browser_mode"),
            browser_ready: state.join("browser_ready"),
            state,
            root,
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn load_config() -> JellyConfig {
    let config_path = repo_root().join("config/jelly.toml");
    match fs::read_to_string(&config_path) {
        Ok(text) => {
            let mut result = toml::from_str::<JellyConfig>(&text).unwrap_or_else(|error| {
                panic!("invalid Jelly config {}: {error}", config_path.display())
            });
            let registry_path = repo_root().join("config/browsers.toml");
            if let Ok(registry) = fs::read_to_string(&registry_path) {
                #[derive(Deserialize)]
                struct Registry {
                    #[serde(default)]
                    remotes: Vec<RemoteBrowserConfig>,
                }
                let registered = toml::from_str::<Registry>(&registry).unwrap_or_else(|error| {
                    panic!(
                        "invalid browser registry {}: {error}",
                        registry_path.display()
                    )
                });
                result.browser.remotes.extend(registered.remotes);
            }
            result
        }
        Err(error) => panic!(
            "cannot read required Jelly config {}: {error}",
            config_path.display()
        ),
    }
}

static CONFIG: LazyLock<JellyConfig> = LazyLock::new(load_config);
thread_local! {
    static ACTIVE_BROWSER_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}
static RUNTIME_PATHS: LazyLock<Mutex<HashMap<String, &'static RuntimePaths>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Apply a browser selection to the current worker thread, restoring it on exit.
pub fn with_browser_id<T>(id: Option<&str>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_BROWSER_ID.with(|selected| *selected.borrow_mut() = self.0.take());
        }
    }
    let prior = ACTIVE_BROWSER_ID.with(|selected| selected.replace(id.map(str::to_owned)));
    let _restore = Restore(prior);
    work()
}

pub fn active_browser_id() -> Option<String> {
    ACTIVE_BROWSER_ID
        .with(|selected| selected.borrow().clone())
        .or_else(|| std::env::var("JELLY_BROWSER_ID").ok())
}

pub fn valid_browser_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// Resolve an ID or easy name to the immutable UUID.
pub fn resolve_remote_id(selector: &str) -> Result<String, String> {
    let remotes = &config().browser.remotes;
    let mut matches = remotes
        .iter()
        .filter(|r| r.id == selector || r.name.as_deref() == Some(selector));
    let selected = matches
        .next()
        .ok_or_else(|| format!("unknown remote browser '{selector}'"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous remote browser selector '{selector}'"));
    }
    if !valid_browser_uuid(&selected.id) {
        return Err(format!(
            "remote browser '{}' requires a valid UUID id",
            selected.id
        ));
    }
    if remotes
        .iter()
        .filter(|r| r.id.eq_ignore_ascii_case(&selected.id))
        .count()
        != 1
    {
        return Err(format!("duplicate remote browser UUID '{}'", selected.id));
    }
    Ok(selected.id.to_ascii_lowercase())
}

pub fn selected_remote_id() -> Result<String, String> {
    let selector = active_browser_id().ok_or_else(|| {
        "set browser_id or JELLY_BROWSER_ID to a configured UUID or easy name".to_string()
    })?;
    resolve_remote_id(&selector)
}

pub fn selected_remote() -> Result<&'static RemoteBrowserConfig, String> {
    let id = selected_remote_id()?;
    let mut matches = config()
        .browser
        .remotes
        .iter()
        .filter(|remote| remote.id.eq_ignore_ascii_case(&id));
    let remote = matches
        .next()
        .ok_or_else(|| format!("unknown remote browser '{id}'"))?;
    if matches.next().is_some() {
        return Err(format!("duplicate remote browser '{id}'"));
    }
    Ok(remote)
}

pub fn selected_remote_endpoint() -> Result<String, String> {
    let endpoint = selected_remote()?.endpoint.trim();
    let url =
        url::Url::parse(endpoint).map_err(|error| format!("invalid remote CDP URL: {error}"))?;
    if !matches!(url.scheme(), "ws" | "wss")
        || url.host_str().is_none()
        || !url.path().starts_with("/devtools/browser/")
    {
        return Err("remote CDP endpoint must be a ws:// or wss:// browser WebSocket URL (/devtools/browser/...)".into());
    }
    Ok(endpoint.to_owned())
}

pub fn config() -> &'static JellyConfig {
    &CONFIG
}

pub fn runtime_paths() -> &'static RuntimePaths {
    let id = if CONFIG.browser.backend == crate::browser_backend::BrowserBackendKind::RemoteCdp {
        selected_remote_id().expect("remote browser identity is required")
    } else {
        String::new()
    };
    let mut guard = RUNTIME_PATHS
        .lock()
        .expect("runtime path registry poisoned");
    guard.entry(id.clone()).or_insert_with(|| {
        let root = if id.is_empty() {
            CONFIG.paths.runtime_root.clone()
        } else {
            CONFIG.paths.runtime_root.join("remote").join(id)
        };
        Box::leak(Box::new(RuntimePaths::from_root(root)))
    })
}

#[derive(Debug, Clone, Copy)]
pub enum RuntimePathKind {
    State,
    Profile,
    HeadlessProfile,
    ArtifactMetadata,
    Screenshots,
    Recordings,
    Downloads,
    Network,
    Logs,
    Routines,
    Injections,
    Endpoint,
    ActiveTarget,
    LogicalTargets,
    LogicalTargetsLock,
    PageTarget,
    BrowserPid,
    BrowserStop,
    BrowserMode,
    BrowserReady,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimePath(pub RuntimePathKind);

impl RuntimePath {
    pub fn path(self) -> &'static Path {
        let paths = runtime_paths();
        match self.0 {
            RuntimePathKind::State => &paths.state,
            RuntimePathKind::Profile => &paths.profile,
            RuntimePathKind::HeadlessProfile => &paths.headless_profile,
            RuntimePathKind::ArtifactMetadata => &paths.artifact_metadata,
            RuntimePathKind::Screenshots => &paths.screenshots,
            RuntimePathKind::Recordings => &paths.recordings,
            RuntimePathKind::Downloads => &paths.downloads,
            RuntimePathKind::Network => &paths.network,
            RuntimePathKind::Logs => &paths.logs,
            RuntimePathKind::Routines => &paths.routines,
            RuntimePathKind::Injections => &paths.injections,
            RuntimePathKind::Endpoint => &paths.endpoint,
            RuntimePathKind::ActiveTarget => &paths.active_target,
            RuntimePathKind::LogicalTargets => &paths.logical_targets,
            RuntimePathKind::LogicalTargetsLock => &paths.logical_targets_lock,
            RuntimePathKind::PageTarget => &paths.page_target,
            RuntimePathKind::BrowserPid => &paths.browser_pid,
            RuntimePathKind::BrowserStop => &paths.browser_stop,
            RuntimePathKind::BrowserMode => &paths.browser_mode,
            RuntimePathKind::BrowserReady => &paths.browser_ready,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.path()
            .to_str()
            .expect("Jelly runtime paths must be valid UTF-8")
    }
}

impl std::ops::Deref for RuntimePath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<Path> for RuntimePath {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl AsRef<std::ffi::OsStr> for RuntimePath {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.path().as_os_str()
    }
}

impl serde::Serialize for RuntimePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl std::fmt::Display for RuntimePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path().display().fmt(f)
    }
}

impl From<&RuntimePath> for String {
    fn from(value: &RuntimePath) -> Self {
        value.as_str().to_owned()
    }
}

impl From<RuntimePath> for PathBuf {
    fn from(value: RuntimePath) -> Self {
        value.path().to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_scoping_restores_previous_selection() {
        with_browser_id(Some("worker-a"), || {
            assert_eq!(active_browser_id().as_deref(), Some("worker-a"));
            with_browser_id(Some("worker-b"), || {
                assert_eq!(active_browser_id().as_deref(), Some("worker-b"));
            });
            assert_eq!(active_browser_id().as_deref(), Some("worker-a"));
        });
    }

    #[test]
    fn rejects_invalid_browser_selection() {
        with_browser_id(Some("../other"), || assert!(selected_remote_id().is_err()));
        assert!(valid_browser_uuid("8f3a0000-1111-4222-8333-44444444c921"));
        assert!(!valid_browser_uuid("worker_2"));
    }

    #[test]
    fn remote_download_policies_deserialize() {
        let base = "id = '8f3a0000-1111-4222-8333-44444444c921'\nendpoint = 'ws://localhost:9222/devtools/browser/x'\ndownload_path = '/downloads'\n";
        let default: RemoteBrowserConfig = toml::from_str(base).unwrap();
        assert_eq!(default.download_mode, RemoteDownloadMode::RemoteOnly);
        let shared: RemoteBrowserConfig = toml::from_str(&format!(
            "{base}download_mode = 'shared-volume'\ndownload_host_path = '/mnt/downloads'\n"
        ))
        .unwrap();
        assert_eq!(shared.download_mode, RemoteDownloadMode::SharedVolume);
        let transfer: RemoteBrowserConfig =
            toml::from_str(&format!("{base}download_mode = 'transfer'\n")).unwrap();
        assert_eq!(transfer.download_mode, RemoteDownloadMode::Transfer);
        assert!(
            toml::from_str::<RemoteBrowserConfig>(&format!("{base}download_mode = 'magic'\n"))
                .is_err()
        );
    }

    #[test]
    fn runtime_paths_derive_from_one_root() {
        let paths = RuntimePaths::from_root(PathBuf::from("/tmp/jelly-runtime"));
        assert_eq!(paths.state, PathBuf::from("/tmp/jelly-runtime/state"));
        assert_eq!(
            paths.screenshots,
            PathBuf::from("/tmp/jelly-runtime/artifacts/screenshots")
        );
        assert_eq!(
            paths.logical_targets,
            PathBuf::from("/tmp/jelly-runtime/state/logical_targets.json")
        );
    }

    #[test]
    fn mcp_connections_are_optional_and_support_multiple_providers() {
        let legacy: McpConfig = toml::from_str(
            "surface = 'small-surface'\npersistent_session = true\nraw_cdp = false\nallow_cargo_fallback = false",
        )
        .unwrap();
        assert!(legacy.connections.is_empty());

        let configured: McpConfig = toml::from_str(
            "surface = 'small-surface'\npersistent_session = true\nraw_cdp = false\nallow_cargo_fallback = false\n\
             [[connections]]\nprovider = 'chatgpt'\nmethod = 'remote-http'\nauth = 'oauth'\n\
             [[connections]]\nprovider = 'generic-mcp'\nmethod = 'local-http'\nauth = 'bearer-token'",
        )
        .unwrap();
        assert_eq!(configured.connections.len(), 2);
        assert_eq!(
            configured.connections[0],
            crate::connection::Provider::ChatGPT.preset()
        );
        assert_eq!(
            configured.connections[1],
            crate::connection::Provider::GenericMcp.preset()
        );
    }

    #[test]
    fn telegram_hitl_format_is_required_configuration() {
        let format = &config().hitl.formats.telegram;
        assert_eq!(format.title, "Jelly");
        assert_eq!(format.subtitle, "Browser instrumentation for agents");
        assert_eq!(format.signature, "⏺️ Recorded with <b>Jelly</b>");
    }
}
