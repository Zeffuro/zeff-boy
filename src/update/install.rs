use super::ReleaseAsset;
use super::strategy::SelfUpdateTarget;
use anyhow::Context;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

pub(super) struct StagedUpdate {
    target: SelfUpdateTarget,
    staged_path: PathBuf,
}

pub(super) fn stage_update(
    asset: &ReleaseAsset,
    target: SelfUpdateTarget,
) -> anyhow::Result<StagedUpdate> {
    let bytes = crate::libretro_common::ureq_get(&asset.url)?
        .read_to_vec()
        .with_context(|| format!("failed to download {}", asset.name))?;
    verify_download(asset, &bytes)?;

    let (staged_path, executable) = match &target {
        SelfUpdateTarget::WindowsPortable(path) => (
            sibling_path(path, "update.exe"),
            executable_from_zip(&bytes)?,
        ),
        SelfUpdateTarget::AppImage(path) => (sibling_path(path, "update"), bytes),
    };
    write_staged_file(&staged_path, &executable)?;

    #[cfg(unix)]
    if matches!(target, SelfUpdateTarget::AppImage(_)) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o755))
            .context("failed to make staged AppImage executable")?;
    }

    Ok(StagedUpdate {
        target,
        staged_path,
    })
}

fn verify_download(asset: &ReleaseAsset, bytes: &[u8]) -> anyhow::Result<()> {
    let actual_sha256 = zeff_firmware::sha256_hex(bytes);
    anyhow::ensure!(
        actual_sha256.eq_ignore_ascii_case(&asset.sha256),
        "downloaded update failed SHA-256 verification"
    );
    Ok(())
}

pub(super) fn activate(staged: &StagedUpdate) -> anyhow::Result<()> {
    match &staged.target {
        SelfUpdateTarget::WindowsPortable(target) => activate_windows(target, &staged.staged_path),
        SelfUpdateTarget::AppImage(target) => activate_appimage(target, &staged.staged_path),
    }
}

fn executable_from_zip(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid update ZIP")?;
    let mut entry = archive
        .by_name("zeff-boy.exe")
        .context("update ZIP does not contain zeff-boy.exe")?;
    let mut executable = Vec::with_capacity(entry.size().try_into().unwrap_or(0));
    entry
        .read_to_end(&mut executable)
        .context("failed to extract zeff-boy.exe")?;
    anyhow::ensure!(!executable.is_empty(), "update executable is empty");
    Ok(executable)
}

fn write_staged_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("failed to stage update at {}", path.display()))?;
    file.write_all(bytes)
        .context("failed to write staged update")?;
    file.sync_all().context("failed to flush staged update")?;
    Ok(())
}

fn sibling_path(target: &Path, suffix: &str) -> PathBuf {
    let stem = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("zeff-boy");
    target.with_file_name(format!("{stem}.{suffix}"))
}

#[cfg(target_os = "windows")]
fn activate_windows(target: &Path, staged: &Path) -> anyhow::Result<()> {
    use std::process::{Command, Stdio};

    let backup = sibling_path(target, "previous.exe");
    let script_path = std::env::temp_dir().join(format!(
        "zeff-boy-update-{}-{}.ps1",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         Wait-Process -Id {} -ErrorAction SilentlyContinue\n\
         try {{\n\
           if (Test-Path -LiteralPath {}) {{ Remove-Item -LiteralPath {} -Force }}\n\
           Move-Item -LiteralPath {} -Destination {} -Force\n\
           Move-Item -LiteralPath {} -Destination {} -Force\n\
           Start-Process -FilePath {}\n\
         }} catch {{\n\
           if ((-not (Test-Path -LiteralPath {})) -and (Test-Path -LiteralPath {})) {{\n\
             Move-Item -LiteralPath {} -Destination {} -Force\n\
           }}\n\
         }} finally {{\n\
           Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force\n\
         }}\n",
        std::process::id(),
        ps_quote(&backup),
        ps_quote(&backup),
        ps_quote(target),
        ps_quote(&backup),
        ps_quote(staged),
        ps_quote(target),
        ps_quote(target),
        ps_quote(target),
        ps_quote(&backup),
        ps_quote(&backup),
        ps_quote(target),
    );
    std::fs::write(&script_path, script).context("failed to create update helper")?;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch update helper")?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn activate_windows(_target: &Path, _staged: &Path) -> anyhow::Result<()> {
    anyhow::bail!("Windows portable updates are not supported on this platform")
}

#[cfg(target_os = "windows")]
fn ps_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

#[cfg(target_os = "linux")]
fn activate_appimage(target: &Path, staged: &Path) -> anyhow::Result<()> {
    let backup = sibling_path(target, "previous.AppImage");
    if backup.exists() {
        std::fs::remove_file(&backup).context("failed to remove previous AppImage backup")?;
    }
    std::fs::rename(target, &backup).context("failed to back up current AppImage")?;
    if let Err(err) = std::fs::rename(staged, target) {
        let _ = std::fs::rename(&backup, target);
        return Err(err).context("failed to activate new AppImage");
    }
    if let Err(err) = std::process::Command::new(target).spawn() {
        let _ = std::fs::remove_file(target);
        let _ = std::fs::rename(&backup, target);
        return Err(err).context("failed to restart updated AppImage");
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn activate_appimage(_target: &Path, _staged: &Path) -> anyhow::Result<()> {
    anyhow::bail!("AppImage updates are not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_the_expected_windows_executable() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file("zeff-boy.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"test executable").unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        assert_eq!(executable_from_zip(&bytes).unwrap(), b"test executable");
    }

    #[test]
    fn staged_paths_stay_next_to_the_running_binary() {
        assert_eq!(
            sibling_path(Path::new("C:/apps/zeff-boy.exe"), "update.exe"),
            PathBuf::from("C:/apps/zeff-boy.update.exe")
        );
    }

    #[test]
    fn downloaded_asset_must_match_the_release_digest() {
        let asset = ReleaseAsset {
            name: "update.zip".to_owned(),
            url: "https://example.com/update.zip".to_owned(),
            sha256: zeff_firmware::sha256_hex(b"expected"),
        };

        verify_download(&asset, b"expected").unwrap();
        assert!(verify_download(&asset, b"tampered").is_err());
    }
}
