use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use anyhow::{Context, Result, ensure};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read as _;

use crate::emu_backend::ActiveSystem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArchiveRomEntry {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) system: ActiveSystem,
    pub(crate) uncompressed_size: u64,
}

impl ArchiveRomEntry {
    pub(crate) fn display_label(&self) -> String {
        format!(
            "{}  ({}, {})",
            self.name,
            system_label(self.system),
            format_size(self.uncompressed_size)
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PendingArchiveSelection {
    pub(crate) archive_path: PathBuf,
    pub(crate) entries: Vec<ArchiveRomEntry>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct BoundedZipMember {
    pub(crate) rom_path: PathBuf,
    pub(crate) member_name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) archive_sha256: [u8; 32],
    pub(crate) archive_len: usize,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct BoundedZipMemberEntry {
    pub(crate) rom_path: PathBuf,
    pub(crate) member_name: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct BoundedZipInspection {
    pub(crate) archive_sha256: [u8; 32],
    pub(crate) entries: Vec<BoundedZipMemberEntry>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn inspect_bounded_zip_members(
    archive_path: &Path,
    extension: &str,
    archive_limit: u64,
    member_limit: u64,
) -> Result<BoundedZipInspection> {
    let archive_bytes = read_file_bounded(archive_path, archive_limit)?;
    let archive_sha256 = zeff_firmware::sha256_bytes(&archive_bytes);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive_bytes))
        .context("failed to read ZIP archive")?;
    ensure!(archive.len() <= 4096, "ZIP contains too many entries");
    let mut entries = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("failed to inspect ZIP entry #{index}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        ensure!(name.len() <= 4096, "ZIP member name is too long");
        ensure!(
            names.insert(name.clone()),
            "ZIP contains duplicate member {name:?}"
        );
        let matches = Path::new(&name)
            .extension()
            .and_then(|candidate| candidate.to_str())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension));
        if !matches {
            continue;
        }
        let relative_path = entry
            .enclosed_name()
            .context("ZIP member path is unsafe")?
            .to_path_buf();
        ensure!(
            entry.size() <= member_limit,
            "ZIP member exceeds the {member_limit}-byte media limit"
        );
        entries.push(BoundedZipMemberEntry {
            rom_path: archive_path.join(relative_path),
            member_name: name,
        });
    }
    Ok(BoundedZipInspection {
        archive_sha256,
        entries,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn extract_bounded_zip_member(
    archive_path: &Path,
    expected_rom_path: Option<&Path>,
    extension: &str,
    archive_limit: u64,
    member_limit: u64,
) -> Result<BoundedZipMember> {
    let archive_bytes = read_file_bounded(archive_path, archive_limit)?;
    let archive_sha256 = zeff_firmware::sha256_bytes(&archive_bytes);
    let archive_len = archive_bytes.len();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive_bytes))
        .context("failed to read ZIP archive")?;
    ensure!(archive.len() <= 4096, "ZIP contains too many entries");

    let expected_name = expected_rom_path
        .map(|path| zip_member_name(archive_path, path))
        .transpose()?;
    let mut candidates = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("failed to inspect ZIP entry #{index}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        ensure!(name.len() <= 4096, "ZIP member name is too long");
        ensure!(
            names.insert(name.clone()),
            "ZIP contains duplicate member {name:?}"
        );
        let is_candidate = Path::new(&name)
            .extension()
            .and_then(|candidate| candidate.to_str())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension));
        if is_candidate
            && expected_name
                .as_ref()
                .is_none_or(|expected| expected == &name)
        {
            ensure!(entry.enclosed_name().is_some(), "ZIP member path is unsafe");
            ensure!(
                entry.size() <= member_limit,
                "ZIP member exceeds the {member_limit}-byte media limit"
            );
            candidates.push(index);
        }
    }

    ensure!(
        candidates.len() == 1,
        "ZIP must resolve to exactly one selected .{extension} member"
    );
    let mut entry = archive
        .by_index(candidates[0])
        .context("failed to reopen selected ZIP member")?;
    let member_name = entry.name().to_owned();
    let relative_path = entry
        .enclosed_name()
        .context("ZIP member path is unsafe")?
        .to_path_buf();
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size())?);
    std::io::Read::by_ref(&mut entry)
        .take(member_limit + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to decompress ZIP member {member_name:?}"))?;
    ensure!(
        bytes.len() as u64 <= member_limit,
        "ZIP member exceeds the {member_limit}-byte media limit"
    );
    Ok(BoundedZipMember {
        rom_path: archive_path.join(relative_path),
        member_name,
        bytes,
        archive_sha256,
        archive_len,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open ZIP archive {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= limit,
        "ZIP exceeds the {limit}-byte limit"
    );
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "ZIP exceeds the {limit}-byte limit"
    );
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn zip_member_name(archive_path: &Path, rom_path: &Path) -> Result<String> {
    let relative = rom_path.strip_prefix(archive_path).with_context(|| {
        format!(
            "selected ROM {} is not inside ZIP {}",
            rom_path.display(),
            archive_path.display()
        )
    })?;
    ensure!(
        !relative.as_os_str().is_empty(),
        "selected ZIP member is empty"
    );
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ArchiveSelectionAction {
    Load {
        archive_path: PathBuf,
        entry_index: usize,
    },
    Cancel,
}

fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;

    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn system_label(system: ActiveSystem) -> &'static str {
    match system {
        ActiveSystem::GameBoy => "Game Boy",
        ActiveSystem::GameBoyAdvance => "Game Boy Advance",
        ActiveSystem::Nes => "NES",
        ActiveSystem::Coleco => "ColecoVision",
        ActiveSystem::Pce => "PC Engine",
        ActiveSystem::WonderSwan => "WonderSwan",
        ActiveSystem::MasterSystem => "Master System",
        ActiveSystem::GameGear => "Game Gear",
        ActiveSystem::Sg1000 => "SG-1000/SC-3000",
    }
}
