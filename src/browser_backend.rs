//! Browser connection and ownership policy.
//! External CDP browsers are never started or terminated by Jelly.
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserBackendKind {
    #[default]
    Native,
    RemoteCdp,
}

pub trait BrowserBackend {
    fn start(&self) -> rustwright::Result<()>;
    fn serve(&self) -> rustwright::Result<()>;
}

pub struct NativeBrowserBackend;
pub struct RemoteCdpBrowserBackend;

impl BrowserBackend for NativeBrowserBackend {
    fn start(&self) -> rustwright::Result<()> {
        crate::browser_launcher::start_native_service()
    }
    fn serve(&self) -> rustwright::Result<()> {
        crate::browser_launcher::serve_native_browser()
    }
}

impl BrowserBackend for RemoteCdpBrowserBackend {
    fn start(&self) -> rustwright::Result<()> {
        crate::browser_launcher::connect_remote_browser()
    }
    fn serve(&self) -> rustwright::Result<()> {
        Err(rustwright::Error::Io(std::io::Error::other(
            "remote CDP has no managed browser service",
        )))
    }
}

pub fn backend_for(kind: BrowserBackendKind) -> &'static dyn BrowserBackend {
    match kind {
        BrowserBackendKind::Native => &NativeBrowserBackend,
        BrowserBackendKind::RemoteCdp => &RemoteCdpBrowserBackend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backend_configuration() {
        let base = "startup_timeout_secs = 30\ncdp_timeout_secs = 60";
        let native: crate::config::BrowserConfig = toml::from_str(base).unwrap();
        assert_eq!(native.backend, BrowserBackendKind::Native);
        let remote: crate::config::BrowserConfig = toml::from_str(
            "startup_timeout_secs = 30\ncdp_timeout_secs = 60\nbackend = 'remote-cdp'",
        )
        .unwrap();
        assert_eq!(remote.backend, BrowserBackendKind::RemoteCdp);
        assert!(
            toml::from_str::<crate::config::BrowserConfig>(
                "startup_timeout_secs = 30\ncdp_timeout_secs = 60\nbackend = 'docker'"
            )
            .is_err()
        );
    }
}
