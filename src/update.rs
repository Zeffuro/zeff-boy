use anyhow::Context;
use serde::Deserialize;
use std::sync::mpsc::{self, Receiver, TryRecvError};

mod install;
mod strategy;

pub(crate) use strategy::UpdateStrategy;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Zeffuro/zeff-boy/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReleaseAsset {
    name: String,
    url: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateInfo {
    pub(crate) version: String,
    pub(crate) release_url: String,
    pub(crate) download_url: String,
    pub(crate) strategy: UpdateStrategy,
    asset: Option<ReleaseAsset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateAction {
    Install,
    Restart,
    Download,
    ReleaseNotes,
    Later,
    SkipVersion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UpdateInstallState {
    #[default]
    Idle,
    Downloading,
    Ready,
}

pub(crate) enum UpdatePoll {
    Available,
    Current,
    CheckFailed(String),
    InstallReady,
    InstallFailed(String),
}

pub(crate) struct UpdateChecker {
    check_receiver: Option<Receiver<anyhow::Result<Option<UpdateInfo>>>>,
    install_receiver: Option<Receiver<anyhow::Result<install::StagedUpdate>>>,
    manual: bool,
    skipped_version: Option<String>,
    available: Option<UpdateInfo>,
    staged: Option<install::StagedUpdate>,
    show_dialog: bool,
    install_state: UpdateInstallState,
}

impl UpdateChecker {
    pub(crate) fn new(check_on_startup: bool, skipped_version: Option<String>) -> Self {
        let mut checker = Self {
            check_receiver: None,
            install_receiver: None,
            manual: false,
            skipped_version,
            available: None,
            staged: None,
            show_dialog: false,
            install_state: UpdateInstallState::Idle,
        };
        if check_on_startup {
            checker.request(false);
        }
        checker
    }

    pub(crate) fn request(&mut self, manual: bool) {
        if self.check_receiver.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.manual = manual;
        self.check_receiver = Some(receiver);
        if let Err(err) = std::thread::Builder::new()
            .name("update-check".to_owned())
            .spawn(move || {
                let _ = sender.send(fetch_latest_release());
            })
        {
            self.check_receiver = None;
            log::warn!("failed to start update check: {err}");
        }
    }

    pub(crate) fn poll(&mut self) -> Option<UpdatePoll> {
        if let Some(receiver) = self.install_receiver.as_ref() {
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    Err(anyhow::anyhow!("update installer stopped unexpectedly"))
                }
            };
            self.install_receiver = None;
            return Some(match result {
                Ok(staged) => {
                    self.staged = Some(staged);
                    self.install_state = UpdateInstallState::Ready;
                    self.show_dialog = true;
                    UpdatePoll::InstallReady
                }
                Err(err) => {
                    self.install_state = UpdateInstallState::Idle;
                    UpdatePoll::InstallFailed(err.to_string())
                }
            });
        }

        let result = match self.check_receiver.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("update check worker stopped unexpectedly"))
            }
        };
        self.check_receiver = None;

        match result {
            Ok(Some(info)) => {
                let skipped =
                    !self.manual && self.skipped_version.as_deref() == Some(info.version.as_str());
                self.available = Some(info);
                self.show_dialog = !skipped;
                (!skipped).then_some(UpdatePoll::Available)
            }
            Ok(None) if self.manual => Some(UpdatePoll::Current),
            Ok(None) => None,
            Err(err) if self.manual => Some(UpdatePoll::CheckFailed(err.to_string())),
            Err(err) => {
                log::warn!("update check failed: {err}");
                None
            }
        }
    }

    pub(crate) fn install(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.install_receiver.is_none(),
            "an update is already downloading"
        );
        let info = self.available.as_ref().context("no update is available")?;
        let asset = info
            .asset
            .clone()
            .context("this release has no self-update asset")?;
        let target = info
            .strategy
            .self_update_target()
            .cloned()
            .context("this installation is managed externally")?;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("update-install".to_owned())
            .spawn(move || {
                let _ = sender.send(install::stage_update(&asset, target));
            })
            .context("failed to start update installer")?;
        self.install_receiver = Some(receiver);
        self.install_state = UpdateInstallState::Downloading;
        Ok(())
    }

    pub(crate) fn activate(&mut self) -> anyhow::Result<()> {
        let staged = self.staged.as_ref().context("no update is ready")?;
        install::activate(staged)?;
        self.staged = None;
        self.install_state = UpdateInstallState::Idle;
        Ok(())
    }

    pub(crate) fn available(&self) -> Option<&UpdateInfo> {
        self.available.as_ref()
    }

    pub(crate) fn show_dialog(&self) -> bool {
        self.show_dialog
    }

    pub(crate) fn install_state(&self) -> UpdateInstallState {
        self.install_state
    }

    pub(crate) fn dismiss(&mut self) {
        self.show_dialog = false;
    }

    pub(crate) fn skip_version(&mut self) -> Option<String> {
        self.show_dialog = false;
        let version = self.available.as_ref()?.version.clone();
        self.skipped_version = Some(version.clone());
        Some(version)
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn fetch_latest_release() -> anyhow::Result<Option<UpdateInfo>> {
    let json = crate::libretro_common::ureq_get_github_json(LATEST_RELEASE_URL)?
        .read_to_string()
        .context("failed to read latest release response")?;
    let release: GithubRelease =
        serde_json::from_str(&json).context("failed to decode latest release response")?;

    if !is_newer_release(&release.tag_name, env!("CARGO_PKG_VERSION"))? {
        return Ok(None);
    }

    let mut strategy = strategy::detect();
    let asset = release
        .assets
        .iter()
        .find(|asset| asset_matches_platform(&asset.name));
    let download_url = asset
        .map(|asset| asset.browser_download_url.clone())
        .unwrap_or_else(|| release.html_url.clone());
    let asset = asset
        .and_then(|asset| {
            let sha256 = asset.digest.as_deref()?.strip_prefix("sha256:")?;
            Some(ReleaseAsset {
                name: asset.name.clone(),
                url: asset.browser_download_url.clone(),
                sha256: sha256.to_owned(),
            })
        })
        .filter(|_| strategy.self_update_target().is_some());
    if strategy.self_update_target().is_some() && asset.is_none() {
        strategy = UpdateStrategy::Browser;
    }
    Ok(Some(UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_owned(),
        release_url: release.html_url,
        download_url,
        strategy,
        asset,
    }))
}

fn is_newer_release(tag: &str, current: &str) -> anyhow::Result<bool> {
    let release = parse_version(tag).context("release tag is not a semantic version")?;
    let current = parse_version(current).context("app version is not a semantic version")?;
    Ok(release > current)
}

fn parse_version(value: &str) -> Option<([u64; 3], bool)> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let (core, prerelease) = value
        .split_once('-')
        .map_or((value, false), |(core, _)| (core, true));
    let mut parts = core.split('.');
    let version = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    if parts.next().is_some() {
        return None;
    }
    Some((version, !prerelease))
}

fn asset_matches_platform(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    return name.ends_with("x86_64-pc-windows-msvc.zip");
    #[cfg(target_os = "macos")]
    return name.ends_with("aarch64-apple-darwin.dmg");
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return name.ends_with("x86_64.AppImage");
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available_update(version: &str) -> UpdateInfo {
        UpdateInfo {
            version: version.to_owned(),
            release_url: "https://example.com/release".to_owned(),
            download_url: "https://example.com/download".to_owned(),
            strategy: UpdateStrategy::Browser,
            asset: None,
        }
    }

    #[test]
    fn compares_release_versions() {
        assert!(is_newer_release("v0.2.0", "0.1.9").unwrap());
        assert!(is_newer_release("v1.0.0", "0.99.99").unwrap());
        assert!(!is_newer_release("v0.1.3", "0.1.3").unwrap());
        assert!(!is_newer_release("v0.1.2", "0.1.3").unwrap());
        assert!(is_newer_release("v0.1.3", "0.1.3-beta.1").unwrap());
    }

    #[test]
    fn rejects_non_semantic_release_tags() {
        assert!(is_newer_release("nightly", "0.1.3").is_err());
        assert!(is_newer_release("v0.1", "0.1.3").is_err());
    }

    #[test]
    fn skipped_version_is_quiet_on_startup_but_visible_when_requested() {
        let mut checker = UpdateChecker::new(false, Some("0.2.0".to_owned()));
        let (sender, receiver) = mpsc::channel();
        checker.check_receiver = Some(receiver);
        sender.send(Ok(Some(available_update("0.2.0")))).unwrap();

        assert!(checker.poll().is_none());
        assert!(!checker.show_dialog());

        let (sender, receiver) = mpsc::channel();
        checker.manual = true;
        checker.check_receiver = Some(receiver);
        sender.send(Ok(Some(available_update("0.2.0")))).unwrap();

        assert!(matches!(checker.poll(), Some(UpdatePoll::Available)));
        assert!(checker.show_dialog());
    }
}
