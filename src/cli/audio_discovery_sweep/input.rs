use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeff_emu_common::system::System;

use super::super::audio_discovery_input::{
    MAX_ARCHIVE_BYTES, MAX_ROM_BYTES_U64, cartridge_input, cartridge_system, display_name,
    minimum_bytes, read_bounded_cartridge_file,
};
use super::evidence;
use super::plans;
use crate::audio_discovery::media::{ContainerIdentity, SelectedMemberIdentity, SourceIdentity};

#[derive(Debug)]
pub(super) struct UnsupportedConfiguration(&'static str);

impl std::fmt::Display for UnsupportedConfiguration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for UnsupportedConfiguration {}

fn require_configuration(condition: bool, message: &'static str) -> Result<()> {
    if !condition {
        return Err(UnsupportedConfiguration(message).into());
    }
    Ok(())
}

pub(super) struct Request {
    pub(super) output: PathBuf,
    pub(super) input: PathBuf,
    pub(super) gb_mode: Option<String>,
    pub(super) sega_video_standard: Option<String>,
    pub(super) sega_console_region: Option<String>,
    pub(super) plans: Vec<plans::Plan>,
    pub(super) plan_document_sha256: Option<String>,
    pub(super) plan_document_path: Option<PathBuf>,
}

pub(super) struct Loaded {
    pub(super) bytes: Arc<[u8]>,
    pub(super) system: System,
    pub(super) source_args: Vec<OsString>,
    pub(super) source: Value,
    pub(super) source_identity: Value,
    pub(super) static_observation: Value,
    pub(super) candidate_evidence: Value,
    pub(super) requested_sha256: String,
    pub(super) sample_rate: u32,
    pub(super) selectors: Value,
}

pub(super) fn parse(args: &[OsString]) -> Result<Option<Request>> {
    if !args.iter().any(|arg| arg == "--audio-capture-sweep") {
        return Ok(None);
    }
    ensure!(
        args.first()
            .is_some_and(|arg| arg == "--audio-capture-sweep"),
        "use --audio-capture-sweep NEW_DIRECTORY INPUT"
    );
    let output = value_path(
        args,
        1,
        "--audio-capture-sweep requires a new output directory",
    )?;
    let input = value_path(args, 2, "--audio-capture-sweep requires an input path")?;
    let mut request = Request {
        output,
        input,
        gb_mode: None,
        sega_video_standard: None,
        sega_console_region: None,
        plans: plans::defaults(),
        plan_document_sha256: None,
        plan_document_path: None,
    };
    let mut index = 3;
    while index < args.len() {
        let flag = args[index]
            .to_str()
            .context("sweep options must be valid Unicode")?;
        let value = || -> Result<String> {
            args.get(index + 1)
                .filter(|value| !value.to_string_lossy().starts_with("--"))
                .and_then(|value| value.to_str())
                .map(str::to_owned)
                .context(format!("{flag} requires a value"))
        };
        match flag {
            "--mode" => {
                ensure!(
                    request.gb_mode.is_none(),
                    "--mode may only be specified once"
                );
                let value = value()?.to_ascii_lowercase();
                ensure!(
                    matches!(value.as_str(), "auto" | "dmg" | "cgb"),
                    "--mode requires auto, dmg, or cgb for capture sweep"
                );
                request.gb_mode = Some(value);
            }
            "--sega8-video-standard" | "--sega8-region" => {
                ensure!(
                    request.sega_video_standard.is_none(),
                    "--sega8-video-standard may only be specified once"
                );
                let value = value()?;
                request.sega_video_standard = Some(if value.eq_ignore_ascii_case("auto") {
                    "auto".to_owned()
                } else {
                    zeff_sega8_core::hardware::timing::Sega8VideoStandard::parse(&value)
                        .context(format!("{flag} requires one of: auto|ntsc|pal|60hz|50hz"))?
                        .label()
                        .to_owned()
                });
            }
            "--sega8-console-region" => {
                ensure!(
                    request.sega_console_region.is_none(),
                    "--sega8-console-region may only be specified once"
                );
                let value = value()?;
                request.sega_console_region = Some(if value.eq_ignore_ascii_case("auto") {
                    "auto".to_owned()
                } else {
                    zeff_sega8_core::hardware::region::Sega8Region::parse(&value)
                        .context(format!("{flag} requires a supported Sega 8-bit region"))?
                        .label()
                        .to_owned()
                });
            }
            "--audio-capture-plans" => {
                ensure!(
                    request.plan_document_sha256.is_none(),
                    "--audio-capture-plans may only be specified once"
                );
                let path = PathBuf::from(value()?);
                let document = plans::load(&path)?;
                request.plan_document_sha256 = Some(document.sha256);
                request.plan_document_path = Some(path);
                request.plans = document.plans;
            }
            _ => anyhow::bail!("unsupported capture sweep option: {flag}"),
        }
        index += 2;
    }
    Ok(Some(request))
}

pub(super) fn load(request: &Request, cancel: &AtomicBool) -> Result<Loaded> {
    let loaded = load_bytes(&request.input)?;
    let bytes = loaded.bytes;
    let system = loaded.system;
    require_configuration(
        !matches!(loaded.extension.as_str(), "fds" | "sgb"),
        "capture sweep requires an ordinary cartridge without FDS or SGB behavior",
    )?;
    require_configuration(
        !matches!(system, System::Gba),
        "capture sweep does not support GBA Direct Sound capture",
    )?;
    require_configuration(
        !(matches!(system, System::Gb) && bytes.get(0x147) == Some(&0xfe)),
        "capture sweep rejects HuC3 because its host-clock mode is not comparable",
    )?;
    if !matches!(system, System::Gb) {
        require_configuration(
            request.gb_mode.is_none(),
            "--mode is only valid for GB/GBC capture sweep",
        )?;
    }
    if !matches!(system, System::Sms | System::Gg | System::Sg) {
        require_configuration(
            request.sega_video_standard.is_none() && request.sega_console_region.is_none(),
            "Sega 8-bit sweep options are only valid for SMS, Game Gear, or SG-1000",
        )?;
    }
    if system == System::Nes {
        require_configuration(
            nes_trace_admitted(&bytes),
            "capture sweep supports only the six native base NES mapper boards",
        )?;
    }
    let source_identity = serde_json::to_value(&loaded.source)?;
    let limits = zeff_audio_discovery::ScanLimits::default();
    let driver_report = zeff_audio_discovery::drivers::scan(system, &bytes, limits, cancel);
    let input = cartridge_input(bytes, loaded.source, system, display_name(&request.input));
    let bytes = input.bytes.clone();
    let manifest = input.analyze(limits, cancel);
    let source = serde_json::to_value(&manifest.source)?;
    let analysis_source = json!({
        "analysis_profile": manifest.analysis_profile,
        "source": manifest.source,
        "transforms": manifest.transforms,
        "media": manifest.scan.media,
    });
    let candidate_evidence = evidence::wrap(
        serde_json::to_value(driver_report)?,
        source_identity.clone(),
        analysis_source,
        serde_json::to_value(manifest.scan.limits)?,
    )?;
    ensure!(
        !cancel.load(std::sync::atomic::Ordering::Relaxed),
        "capture sweep cancelled while loading static candidate evidence"
    );
    let static_observation =
        serde_json::to_value(zeff_audio_discovery::coverage::observe(&manifest.scan))?;
    Ok(Loaded {
        bytes,
        system,
        source_args: loaded.source_args,
        source,
        source_identity,
        static_observation,
        candidate_evidence,
        requested_sha256: loaded.requested_sha256,
        sample_rate: if system == System::Pce {
            44_100
        } else {
            48_000
        },
        selectors: serde_json::json!({
            "gb_mode": request.gb_mode.as_deref().unwrap_or("auto"),
            "sega_video_standard": request.sega_video_standard.as_deref().unwrap_or("auto"),
            "sega_console_region": request.sega_console_region.as_deref().unwrap_or("auto"),
        }),
    })
}

pub(super) fn source_is_unchanged(path: &Path, expected_sha256: &str) -> bool {
    requested_sha256(path).is_ok_and(|hash| hash == expected_sha256)
}

struct LoadedBytes {
    bytes: Vec<u8>,
    system: System,
    source_args: Vec<OsString>,
    requested_sha256: String,
    source: SourceIdentity,
    extension: String,
}

fn load_bytes(path: &Path) -> Result<LoadedBytes> {
    if let Some(system) = cartridge_system(path) {
        let bytes = read_bounded_cartridge_file(path, system)?;
        let hash = zeff_firmware::sha256_hex(&bytes);
        let extension = extension(path)?;
        return Ok(LoadedBytes {
            source: SourceIdentity {
                kind: "direct_cartridge_file",
                sha256: hash.clone(),
                len: bytes.len(),
                container: None,
                selected_member: None,
            },
            bytes,
            system,
            source_args: vec![path.as_os_str().to_owned()],
            requested_sha256: hash,
            extension,
        });
    }
    ensure!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip")),
        "capture sweep requires a direct cartridge or a ZIP containing exactly one supported cartridge"
    );
    let entries = zip_candidates(path)?;
    ensure!(
        entries.candidates == 1,
        "capture sweep ZIP must contain exactly one supported cartridge"
    );
    let (member, system) = entries
        .selected
        .context("capture sweep ZIP selected media is not an ordinary cartridge")?;
    let extension = extension(Path::new(&member))?;
    let extracted = crate::rom_archive::extract_single_authenticated_bounded_zip_member(
        path,
        &extension,
        MAX_ARCHIVE_BYTES,
        MAX_ROM_BYTES_U64,
    )?;
    ensure!(
        (minimum_bytes(system)..=MAX_ROM_BYTES_U64).contains(&(extracted.bytes.len() as u64)),
        "ZIP cartridge member is outside the supported size bounds"
    );
    let (archive_sha256, archive_len, member_name) = extracted.witness.archive_identity();
    ensure!(
        entries.sha256 == const_hex::encode(archive_sha256),
        "ZIP changed after its candidate inventory"
    );
    let source_sha256 = zeff_firmware::sha256_hex(&extracted.bytes);
    Ok(LoadedBytes {
        source: SourceIdentity {
            kind: "zip_member",
            sha256: source_sha256.clone(),
            len: extracted.bytes.len(),
            container: Some(ContainerIdentity {
                format: "zip",
                sha256: const_hex::encode(archive_sha256),
                len: archive_len,
            }),
            selected_member: Some(SelectedMemberIdentity {
                name: member_name.to_owned(),
                sha256: source_sha256,
                len: extracted.bytes.len(),
            }),
        },
        bytes: extracted.bytes,
        system,
        source_args: vec![path.as_os_str().to_owned()],
        requested_sha256: const_hex::encode(archive_sha256),
        extension,
    })
}

pub(super) struct ZipCandidates {
    pub(super) candidates: usize,
    selected: Option<(String, System)>,
    sha256: String,
}

pub(super) fn zip_candidates(path: &Path) -> Result<ZipCandidates> {
    let bytes = read_bounded(path, MAX_ARCHIVE_BYTES)?;
    let sha256 = zeff_firmware::sha256_hex(&bytes);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    ensure!(archive.len() <= 4096, "ZIP contains too many entries");
    let mut names = std::collections::BTreeSet::new();
    let mut candidates = 0;
    let mut selected = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name();
        ensure!(entry.enclosed_name().is_some(), "ZIP member path is unsafe");
        ensure!(
            names.insert(name.to_owned()),
            "ZIP contains duplicate member {name:?}"
        );
        if crate::emu_backend::ActiveSystem::from_path(Path::new(name)).is_some() {
            candidates += 1;
            if let Some(system) = cartridge_system(Path::new(name)) {
                selected = Some((name.to_owned(), system));
            }
        }
    }
    Ok(ZipCandidates {
        candidates,
        selected,
        sha256,
    })
}

fn extension(path: &Path) -> Result<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .context("cartridge has no usable extension")
}

fn requested_sha256(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect requested source {}", path.display()))?;
    ensure!(
        metadata.is_file(),
        "requested source must be a regular file"
    );
    ensure!(
        metadata.len() <= MAX_ARCHIVE_BYTES,
        "requested source exceeds the capture sweep limit"
    );
    let expected_len = metadata.len();
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open requested source {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        ensure!(
            bytes <= MAX_ARCHIVE_BYTES,
            "requested source exceeds the capture sweep limit"
        );
        digest.update(&buffer[..count]);
    }
    ensure!(
        bytes == expected_len && std::fs::metadata(path)?.len() == expected_len,
        "requested source changed while it was read"
    );
    Ok(const_hex::encode(digest.finalize()))
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect input {}", path.display()))?;
    ensure!(metadata.is_file(), "input must be a regular file");
    ensure!(
        metadata.len() <= limit,
        "input exceeds the capture sweep limit"
    );
    let expected_len = usize::try_from(metadata.len()).context("input is too large")?;
    let mut bytes = Vec::with_capacity(expected_len);
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() == expected_len && std::fs::metadata(path)?.len() == metadata.len(),
        "input changed while it was read"
    );
    Ok(bytes)
}

pub(super) fn nes_trace_admitted(bytes: &[u8]) -> bool {
    zeff_nes_core::emulator::Emulator::new_with_audio_trace(bytes, 48_000.0, 1).is_ok()
}

fn value_path(args: &[OsString], index: usize, error: &'static str) -> Result<PathBuf> {
    let value = args.get(index).context(error)?;
    ensure!(!value.to_string_lossy().starts_with("--"), "{error}");
    Ok(value.into())
}
