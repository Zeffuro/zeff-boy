#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelfUpdateTarget {
    WindowsPortable(PathBuf),
    AppImage(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateStrategy {
    SelfUpdate(SelfUpdateTarget),
    PackageManager {
        name: &'static str,
        command: Option<String>,
    },
    Browser,
}

impl UpdateStrategy {
    pub(crate) fn self_update_target(&self) -> Option<&SelfUpdateTarget> {
        match self {
            Self::SelfUpdate(target) => Some(target),
            Self::PackageManager { .. } | Self::Browser => None,
        }
    }
}

pub(super) fn detect() -> UpdateStrategy {
    if cfg!(debug_assertions) {
        return UpdateStrategy::Browser;
    }

    #[cfg(target_os = "windows")]
    return detect_windows();
    #[cfg(target_os = "linux")]
    return detect_linux();
    #[cfg(target_os = "macos")]
    return detect_macos();
    #[allow(unreachable_code)]
    UpdateStrategy::Browser
}

#[cfg(target_os = "windows")]
fn detect_windows() -> UpdateStrategy {
    let Ok(exe) = std::env::current_exe() else {
        return UpdateStrategy::Browser;
    };
    windows_strategy_for_exe(exe)
}

#[cfg(target_os = "windows")]
fn windows_strategy_for_exe(exe: PathBuf) -> UpdateStrategy {
    let normalized = exe
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    if normalized.contains("\\scoop\\apps\\zeff-boy\\") {
        return package("Scoop", "scoop update zeff-boy");
    }
    if normalized.contains("\\chocolatey\\lib\\zeff-boy\\") {
        return package("Chocolatey", "choco upgrade zeff-boy");
    }
    if normalized.contains("\\microsoft\\winget\\packages\\") {
        return package("winget", "winget upgrade --id Zeffuro.ZeffBoy");
    }
    UpdateStrategy::SelfUpdate(SelfUpdateTarget::WindowsPortable(exe))
}

#[cfg(target_os = "linux")]
fn detect_linux() -> UpdateStrategy {
    if let Some(path) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        return UpdateStrategy::SelfUpdate(SelfUpdateTarget::AppImage(path));
    }
    if std::env::var_os("FLATPAK_ID").is_some() {
        return package("Flatpak", "flatpak update com.github.zeffuro.zeff-boy");
    }
    if std::env::var_os("SNAP").is_some() {
        return package("Snap", "sudo snap refresh zeff-boy");
    }

    let exe = std::env::current_exe().unwrap_or_default();
    if exe.starts_with("/nix/store") {
        return UpdateStrategy::PackageManager {
            name: "Nix",
            command: None,
        };
    }
    if exe.starts_with("/usr/bin") || exe.starts_with("/usr/local/bin") {
        if Path::new("/etc/arch-release").exists() {
            let helper = ["paru", "yay"]
                .into_iter()
                .find(|name| command_exists(name))
                .unwrap_or("your AUR helper");
            let command =
                (helper != "your AUR helper").then(|| format!("{helper} -Syu zeff-boy-bin"));
            return UpdateStrategy::PackageManager {
                name: "AUR",
                command,
            };
        }
        return UpdateStrategy::PackageManager {
            name: "system package manager",
            command: None,
        };
    }
    UpdateStrategy::Browser
}

#[cfg(target_os = "macos")]
fn detect_macos() -> UpdateStrategy {
    let exe = std::env::current_exe().unwrap_or_default();
    let path = exe.to_string_lossy();
    if path.contains("/Cellar/") || path.contains("/Caskroom/") {
        return package("Homebrew", "brew upgrade --cask zeff-boy");
    }
    UpdateStrategy::Browser
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn package(name: &'static str, command: &str) -> UpdateStrategy {
    UpdateStrategy::PackageManager {
        name,
        command: Some(command.to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_update_targets_are_exposed_only_for_portable_strategies() {
        let target = SelfUpdateTarget::WindowsPortable(PathBuf::from("zeff-boy.exe"));
        assert!(
            UpdateStrategy::SelfUpdate(target)
                .self_update_target()
                .is_some()
        );
        assert!(
            UpdateStrategy::PackageManager {
                name: "test",
                command: None,
            }
            .self_update_target()
            .is_none()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_package_manager_paths_are_not_self_updated() {
        for (path, name) in [
            (
                r"C:\Users\test\scoop\apps\zeff-boy\current\zeff-boy.exe",
                "Scoop",
            ),
            (
                r"C:\ProgramData\chocolatey\lib\zeff-boy\tools\zeff-boy.exe",
                "Chocolatey",
            ),
            (
                r"C:\Users\test\AppData\Local\Microsoft\WinGet\Packages\Zeffuro.ZeffBoy\zeff-boy.exe",
                "winget",
            ),
        ] {
            assert!(matches!(
                windows_strategy_for_exe(PathBuf::from(path)),
                UpdateStrategy::PackageManager { name: actual, .. } if actual == name
            ));
        }

        assert!(matches!(
            windows_strategy_for_exe(PathBuf::from(r"D:\Apps\zeff-boy.exe")),
            UpdateStrategy::SelfUpdate(SelfUpdateTarget::WindowsPortable(_))
        ));
    }
}
