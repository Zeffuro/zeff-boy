use serde::Deserialize;
use zeff_netplay::wire::BuildInfo;

use crate::emu_backend::EmuBackend;

const QUALIFICATION: &str = include_str!("qualification.json");
const HARDWARE: &str = "nes-nrom-ntsc-v1";

mod certificate;
mod certificate_fp;
pub(crate) use certificate::CertificateSummary;
pub(crate) use certificate_fp::floating_point_controls;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    format: u32,
    builds: Vec<QualifiedBuild>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualifiedBuild {
    source: String,
    receipt: String,
    platform: u8,
    test: bool,
    hardware: String,
    contract: String,
}

fn digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    const_hex::decode_to_array(value).ok()
}

fn platform() -> u8 {
    if !cfg!(all(
        target_arch = "x86_64",
        target_endian = "little",
        target_pointer_width = "64"
    )) {
        return 0;
    }
    if cfg!(all(target_os = "windows", target_env = "msvc")) {
        1
    } else if cfg!(all(target_os = "linux", target_env = "gnu")) {
        2
    } else {
        0
    }
}

fn qualified_contract(
    table: &str,
    source: &str,
    receipt: Option<&str>,
    platform: u8,
    test: bool,
) -> Option<[u8; 32]> {
    let source = digest(source)?;
    let receipt = digest(receipt?)?;
    if !matches!(platform, 1 | 2) || table.len() > 32 * 1024 {
        return None;
    }
    let table: Table = serde_json::from_str(table).ok()?;
    if table.format != 1 || table.builds.len() > 64 {
        return None;
    }
    let mut selected = None;
    for build in table.builds {
        let contract = digest(&build.contract)?;
        let row_source = digest(&build.source)?;
        let row_receipt = digest(&build.receipt)?;
        if !matches!(build.platform, 1 | 2) || build.hardware != HARDWARE || contract == [0; 32] {
            return None;
        }
        if row_source == source
            && row_receipt == receipt
            && build.platform == platform
            && build.test == test
        {
            if selected.is_some() {
                return None;
            }
            selected = Some(contract);
        }
    }
    selected
}

fn supported_hardware(backend: &EmuBackend) -> bool {
    let Some(nes) = backend.nes() else {
        return false;
    };
    let header = nes.emu.cartridge_header();
    nes.emu.has_ntsc_timing()
        && header.mapper_id == 0
        && header.submapper_id == 0
        && nes.emu.cartridge_effective_mapper_label() == header.mapper_label()
}

fn compiled_contract() -> Option<[u8; 32]> {
    qualified_contract(
        QUALIFICATION,
        env!("ZEFF_NETPLAY_QUALIFICATION_SOURCE"),
        option_env!("ZEFF_NETPLAY_BUILD_RECEIPT_V1"),
        platform(),
        cfg!(test),
    )
}

pub(crate) fn available() -> bool {
    compiled_contract().is_some()
        || (certificate_fp::supported(floating_point_controls())
            && local_pair()
                .ok()
                .flatten()
                .is_some_and(|pair| pair.different_versions()))
}

fn local_pair() -> anyhow::Result<Option<certificate::VerifiedPair>> {
    let path = std::env::current_exe()?.with_extension("netplay.json");
    let Some(bytes) = certificate::read_sidecar(&path)? else {
        return Ok(None);
    };
    let facts = certificate::LocalFacts {
        artifact: super::connect::executable_build()?,
        source: env!("ZEFF_NETPLAY_QUALIFICATION_SOURCE"),
        version: env!("CARGO_PKG_VERSION"),
        target: env!("ZEFF_NETPLAY_BUILD_TARGET"),
        test: cfg!(test),
        profile: env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        opt_level: env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        debug: env!("ZEFF_NETPLAY_BUILD_DEBUG"),
    };
    certificate::verify(&bytes, &facts).map(Some)
}

pub(crate) fn certificate_status() -> anyhow::Result<Option<CertificateSummary>> {
    local_pair().map(|pair| pair.map(|pair| pair.summary()))
}

pub(crate) fn signed_pair_available() -> bool {
    certificate_fp::supported(floating_point_controls()) && local_pair().ok().flatten().is_some()
}

pub(crate) fn describe(backend: &EmuBackend, allow_different_versions: bool) -> BuildInfo {
    let mut result = BuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        allow_different_versions,
        ..BuildInfo::default()
    };
    if certificate_fp::supported(floating_point_controls())
        && let Ok(Some(pair)) = local_pair()
        && let Some(nes) = backend.nes()
        && let Some(provenance) = backend.nes_tas_load_provenance()
    {
        use zeff_nes_core::hardware::cartridge::TimingMode;
        let header = nes.emu.cartridge_header();
        let timing = match nes.emu.resolved_timing_mode() {
            TimingMode::Ntsc => Some(0),
            TimingMode::Pal => Some(1),
            TimingMode::Dendy => Some(2),
            TimingMode::MultiRegion => None,
        };
        if provenance.load.raw_source_media_sha256 == backend.rom_hash()
            && nes.emu.cartridge_effective_mapper_label() == header.mapper_label()
            && timing.is_some_and(|timing| {
                pair.permits_content(
                    backend.rom_hash(),
                    header.mapper_id,
                    header.submapper_id,
                    timing,
                    nes.emu.has_portable_rollback_hardware(),
                )
            })
        {
            let summary = pair.summary();
            result.contract = summary.contract;
            result.certificate = summary.certificate;
            result.peer_build = summary.peer_build;
            result.platform = platform();
            return result;
        }
    }
    if supported_hardware(backend)
        && let Some(contract) = compiled_contract()
    {
        result.contract = contract;
        result.platform = platform();
    }
    result
}

#[cfg(test)]
mod tests;
