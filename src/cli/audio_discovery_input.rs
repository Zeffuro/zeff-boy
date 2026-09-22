use std::ffi::OsString;
use std::io::Read;
use std::path::{Component, Path};
use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Context, ensure};
use zeff_emu_common::system::System;

use crate::audio_discovery::MAX_ROM_BYTES;
use crate::audio_discovery::cdda::{CdAudioInput, CdAudioProvenance};
use crate::audio_discovery::media::{
    ContainerIdentity, ScanInput, ScanProvenance, SelectedMemberIdentity, SourceIdentity,
    StandaloneFormat,
};
use crate::audio_discovery::tracker::EmbeddedFormat;

pub(super) const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
pub(super) const MIN_GBA_ROM_BYTES: u64 = 0xC0;
pub(super) const MAX_ROM_BYTES_U64: u64 = MAX_ROM_BYTES as u64;

use super::AudioDiscoveryRequest;

pub(super) fn load_input(request: &AudioDiscoveryRequest) -> anyhow::Result<ScanInput> {
    ensure_distinct_output_path(&request.output_path, &request.input_path)?;
    if has_extension(&request.input_path, "cue") {
        ensure!(
            request.archive_member.is_none(),
            "--archive-member is only valid when the input is a ZIP archive"
        );
        let loaded =
            crate::emu_backend::pce_cd::load_direct_cue_with_mods(&request.input_path, false)?;
        return disc_input(loaded, "cue", None, display_name(&request.input_path));
    }
    if audio_format(&request.input_path).is_some() {
        ensure!(
            request.archive_member.is_none(),
            "--archive-member is only valid when the input is a ZIP archive"
        );
        return crate::audio_discovery::media::import::load_audio(&request.input_path);
    }
    if let Some(system) = cartridge_system(&request.input_path) {
        ensure!(
            request.archive_member.is_none(),
            "--archive-member is only valid when the input is a ZIP archive"
        );
        let bytes = read_bounded_cartridge_file(&request.input_path, system)?;
        let source = SourceIdentity {
            kind: if system == System::Gba {
                "direct_gba_file"
            } else {
                "direct_cartridge_file"
            },
            sha256: sha256_hex(&bytes),
            len: bytes.len(),
            container: None,
            selected_member: None,
        };
        return Ok(cartridge_input(
            bytes,
            source,
            system,
            display_name(&request.input_path),
        ));
    }

    ensure!(
        has_extension(&request.input_path, "zip"),
        "audio discovery supports cartridge files, standalone XM/MOD/S3M/IT/VGM/VGZ/GBS/NSF files, direct .cue sets, or a ZIP with an explicitly selected cartridge, audio file, or .cue member; CHD and ISO input are not supported here"
    );
    let member = request
        .archive_member
        .as_deref()
        .context("ZIP audio discovery requires --archive-member <cartridge-module-or-cue>")?;
    if has_extension(Path::new(member), "cue") {
        let (_, loaded, identity) = crate::emu_backend::pce_cd_zip::load_zip_selected_cue_with_control_and_archive_identity(
            &request.input_path, member, Arc::new(AtomicBool::new(false)),
            Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default()), false,
        )?;
        return disc_input(
            loaded,
            "zip_cue",
            Some(identity),
            display_name(Path::new(member)),
        );
    }
    if let Some(format) = audio_format(Path::new(member)) {
        return standalone_zip_input(&request.input_path, member, format);
    }
    let system = cartridge_system(Path::new(member))
        .context("selected ZIP member is not a supported cartridge")?;
    let extension = Path::new(member)
        .extension()
        .and_then(|ext| ext.to_str())
        .context("selected ZIP member has no extension")?
        .to_ascii_lowercase();
    let expected_rom_path = request.input_path.join(member);
    let extracted = crate::rom_archive::extract_authenticated_bounded_zip_member(
        &request.input_path,
        Some(&expected_rom_path),
        &extension,
        MAX_ARCHIVE_BYTES,
        MAX_ROM_BYTES_U64,
    )?;
    ensure!(
        (minimum_bytes(system)..=MAX_ROM_BYTES_U64).contains(&(extracted.bytes.len() as u64)),
        "selected ZIP cartridge member must be between {} and {MAX_ROM_BYTES_U64} bytes",
        minimum_bytes(system)
    );
    let (archive_sha256, archive_len, member_name) = extracted.witness.archive_identity();
    let source_sha256 = sha256_hex(&extracted.bytes);
    let source = SourceIdentity {
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
    };
    Ok(cartridge_input(
        extracted.bytes,
        source,
        system,
        display_name(Path::new(member)),
    ))
}

pub(super) fn cartridge_input(
    bytes: Vec<u8>,
    source: SourceIdentity,
    system: System,
    display_name: Option<String>,
) -> ScanInput {
    ScanInput {
        cdda: None,
        system: Some(system),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: Some(Arc::new(ScanProvenance {
            source,
            transforms: Vec::new(),
        })),
        analysis_profile: "standalone-unmodified-v1",
        display_name,
    }
}

pub(super) fn standalone_zip_input(
    archive_path: &Path,
    member: &str,
    format: StandaloneFormat,
) -> anyhow::Result<ScanInput> {
    let extension = Path::new(member)
        .extension()
        .and_then(|value| value.to_str())
        .context("selected audio member has no extension")?
        .to_ascii_lowercase();
    let extracted = crate::rom_archive::extract_authenticated_bounded_zip_member(
        archive_path,
        Some(&archive_path.join(member)),
        &extension,
        MAX_ARCHIVE_BYTES,
        MAX_ROM_BYTES_U64,
    )?;
    ensure!(
        !extracted.bytes.is_empty(),
        "selected ZIP standalone audio file must not be empty"
    );
    let (archive_sha256, archive_len, member_name) = extracted.witness.archive_identity();
    let source_sha256 = sha256_hex(&extracted.bytes);
    let member_len = extracted.bytes.len();
    Ok(ScanInput::standalone(
        extracted.bytes,
        SourceIdentity {
            kind: match format {
                StandaloneFormat::Tracker(EmbeddedFormat::Xm) => "zip_xm_member",
                StandaloneFormat::Tracker(EmbeddedFormat::Mod) => "zip_mod_member",
                StandaloneFormat::Tracker(EmbeddedFormat::S3m) => "zip_s3m_member",
                StandaloneFormat::Tracker(EmbeddedFormat::It) => "zip_it_member",
                StandaloneFormat::Vgm => "zip_vgm_member",
                StandaloneFormat::Rip(crate::audio_discovery::rips::RipFormat::Gbs) => {
                    "zip_gbs_member"
                }
                StandaloneFormat::Rip(crate::audio_discovery::rips::RipFormat::Nsf) => {
                    "zip_nsf_member"
                }
            },
            sha256: source_sha256.clone(),
            len: member_len,
            container: Some(ContainerIdentity {
                format: "zip",
                sha256: const_hex::encode(archive_sha256),
                len: archive_len,
            }),
            selected_member: Some(SelectedMemberIdentity {
                name: member_name.to_owned(),
                sha256: source_sha256,
                len: member_len,
            }),
        },
        format,
        display_name(Path::new(member)),
    ))
}

pub(super) fn disc_input(
    loaded: crate::emu_backend::pce_cd::LoadedPceCd,
    kind: &'static str,
    archive: Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    display_name: Option<String>,
) -> anyhow::Result<ScanInput> {
    let effective_hash = const_hex::encode(loaded.disc.content_hash());
    let original_hash = const_hex::encode(loaded.source_disc_sha256);
    ensure!(
        original_hash == effective_hash,
        "standalone CD audio loading unexpectedly changed the disc"
    );
    let original_len = loaded
        .disc
        .payload_len()
        .context("CD payload length exceeds this platform")?;
    let provenance = CdAudioProvenance {
        source_kind: kind,
        source_media_sha256: const_hex::encode(
            archive.map_or(loaded.raw_source_media_sha256, |identity| {
                identity.source_sha256
            }),
        ),
        source_media_len: archive
            .map_or(loaded.raw_source_media_len, |identity| identity.source_len),
        selected_member_path_sha256: archive
            .map(|identity| const_hex::encode(identity.cue_member_path_sha256)),
        transforms_applied: false,
    };
    let mut input = ScanInput::from_disc(
        CdAudioInput::new(
            Arc::new(loaded.disc),
            original_hash,
            Some(original_len),
            effective_hash,
            provenance,
        )?,
        "standalone-unmodified-v1",
    );
    input.display_name = display_name;
    Ok(input)
}

pub(super) fn cartridge_system(path: &Path) -> Option<System> {
    if ["cue", "chd", "iso"]
        .iter()
        .any(|extension| has_extension(path, extension))
    {
        return None;
    }
    System::from_path(path)
}

#[cfg(test)]
pub(super) fn standalone_format(path: &Path) -> Option<EmbeddedFormat> {
    crate::audio_discovery::media::import::tracker_format(path)
}

pub(super) fn audio_format(path: &Path) -> Option<StandaloneFormat> {
    crate::audio_discovery::media::import::audio_format(path)
}

pub(super) fn display_name(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn minimum_bytes(system: System) -> u64 {
    if system == System::Gba {
        MIN_GBA_ROM_BYTES
    } else {
        1
    }
}

pub(super) fn read_bounded_cartridge_file(path: &Path, system: System) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect cartridge input {}", path.display()))?;
    ensure!(metadata.is_file(), "cartridge input must be a regular file");
    ensure!(
        (minimum_bytes(system)..=MAX_ROM_BYTES_U64).contains(&metadata.len()),
        "cartridge input must be between {} and {MAX_ROM_BYTES_U64} bytes",
        minimum_bytes(system)
    );
    let expected_len = usize::try_from(metadata.len()).context("cartridge input is too large")?;
    let mut bytes = Vec::with_capacity(expected_len);
    std::fs::File::open(path)
        .with_context(|| format!("failed to open cartridge input {}", path.display()))?
        .take(MAX_ROM_BYTES_U64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read cartridge input {}", path.display()))?;
    ensure!(
        bytes.len() == expected_len,
        "cartridge input changed while it was read"
    );
    Ok(bytes)
}

pub(super) fn normalize_archive_member(member: &str) -> anyhow::Result<String> {
    let member = member.replace('\\', "/");
    let path = Path::new(&member);
    ensure!(!member.is_empty(), "--archive-member must not be empty");
    ensure!(
        !path.is_absolute()
            && !member.contains(':')
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "--archive-member must be a relative ZIP member path without traversal"
    );
    ensure!(
        cartridge_system(path).is_some()
            || audio_format(path).is_some()
            || has_extension(path, "cue"),
        "--archive-member must name a supported cartridge, XM/MOD/S3M/IT/VGM/VGZ/GBS/NSF, or .cue ZIP member"
    );
    Ok(member)
}

pub(super) fn parse_u64(value: &str, flag: &str) -> anyhow::Result<u64> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse(), |hex| u64::from_str_radix(hex, 16));
    parsed.map_err(|_| anyhow::anyhow!("{flag} must be an unsigned integer"))
}

pub(super) fn parse_u32(value: &str, flag: &str) -> anyhow::Result<u32> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse(), |hex| u32::from_str_radix(hex, 16));
    parsed.map_err(|_| anyhow::anyhow!("{flag} must be an unsigned integer"))
}

pub(super) fn required_audio_u32(
    args: &[OsString],
    index: usize,
    flag: &str,
) -> anyhow::Result<u32> {
    let value = args
        .get(index)
        .context(format!("{flag} requires an unsigned integer"))?
        .to_str()
        .context(format!("{flag} must be valid Unicode"))?;
    ensure!(
        !value.starts_with("--"),
        "{flag} requires an unsigned integer"
    );
    parse_u32(value, flag)
}

pub(super) fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    const_hex::encode(zeff_firmware::sha256_bytes(bytes))
}

pub(super) fn required_path_value<'a>(
    args: &'a [OsString],
    index: usize,
    error: &'static str,
) -> anyhow::Result<&'a OsString> {
    let value = args.get(index).context(error)?;
    ensure!(!value.to_string_lossy().starts_with("--"), "{error}");
    Ok(value)
}

pub(in crate::cli) fn ensure_distinct_output_path(
    output: &Path,
    input: &Path,
) -> anyhow::Result<()> {
    let output = resolved_path(output)?;
    let input = resolved_path(input)?;
    #[cfg(windows)]
    let same = output.to_string_lossy().to_lowercase() == input.to_string_lossy().to_lowercase();
    #[cfg(not(windows))]
    let same = output == input;
    ensure!(
        !same,
        "audio input, scan report, and export paths must be distinct"
    );
    Ok(())
}

pub(in crate::cli) fn ensure_outside_output_directory(
    output: &Path,
    directory: &Path,
) -> anyhow::Result<()> {
    let output = resolved_path(output)?;
    let directory = resolved_path(directory)?;
    #[cfg(windows)]
    let (output, directory) = (
        std::path::PathBuf::from(output.to_string_lossy().to_lowercase()),
        std::path::PathBuf::from(directory.to_string_lossy().to_lowercase()),
    );
    ensure!(
        !output.starts_with(directory),
        "audio output must be outside other output directories"
    );
    Ok(())
}

fn resolved_path(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in suffix.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(
                    ancestor
                        .file_name()
                        .context("audio output has no resolvable parent")?
                        .to_owned(),
                );
                ancestor = ancestor
                    .parent()
                    .context("audio output has no resolvable parent")?;
            }
            Err(error) => return Err(error).context("could not resolve audio output path"),
        }
    }
}
