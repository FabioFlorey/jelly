use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
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
    pub startup_timeout_secs: u64,
    pub cdp_timeout_secs: u64,
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
        Ok(text) => toml::from_str::<JellyConfig>(&text).unwrap_or_else(|error| {
            panic!("invalid Jelly config {}: {error}", config_path.display())
        }),
        Err(error) => panic!(
            "cannot read required Jelly config {}: {error}",
            config_path.display()
        ),
    }
}

static CONFIG: LazyLock<JellyConfig> = LazyLock::new(load_config);
static RUNTIME_PATHS: LazyLock<RuntimePaths> =
    LazyLock::new(|| RuntimePaths::from_root(CONFIG.paths.runtime_root.clone()));

pub fn config() -> &'static JellyConfig {
    &CONFIG
}

pub fn runtime_paths() -> &'static RuntimePaths {
    &RUNTIME_PATHS
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
