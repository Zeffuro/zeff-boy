use anyhow::{Context, Result, ensure};

use super::{EmuBackend, TasDigest, TasExternalIdentity};

pub(super) const GB_TAS_RTC_EPOCH_UNIX_SECONDS: u64 = 946_684_800;
const GB_RTC_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-mbc3-rtc\0clock=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00Z\0boot=internal-post-boot\0serial=disconnected\0mods=disabled\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0";
const GB_RTC_STATE_CONFIGURATION: &[u8] =
    b"zeff-gb-tas-rtc-state-v1\0clock=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00Z\0";

#[derive(Clone, Copy)]
pub(super) enum GbTasRtcHardware {
    Dmg,
    Cgb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GbRtcPersistenceWitness {
    pub(crate) persistent_state: TasExternalIdentity,
    pub(crate) rtc_state: TasExternalIdentity,
    pub(crate) complete_byte_len: u64,
    pub(crate) complete_sha256: TasDigest,
}

pub(super) fn gb_rtc_sync_config_sha256(
    hardware: GbTasRtcHardware,
    ram_len: usize,
    member_name: Option<&str>,
) -> TasDigest {
    let mut bytes = Vec::from(GB_RTC_SYNC_CONFIGURATION);
    bytes.extend_from_slice(match hardware {
        GbTasRtcHardware::Dmg => b"hardware=dmg\0".as_slice(),
        GbTasRtcHardware::Cgb => b"hardware=cgb\0media=cgb-exclusive\0".as_slice(),
    });
    bytes.extend_from_slice(b"persistent-state=project-owned-mbc3-ram\0ram-len=");
    bytes.extend_from_slice(ram_len.to_string().as_bytes());
    bytes.push(0);
    if let Some(member_name) = member_name {
        bytes.extend_from_slice(b"route=zip-member\0member=");
        bytes.extend_from_slice(member_name.as_bytes());
    } else {
        bytes.extend_from_slice(b"route=direct\0");
    }
    TasDigest::from_bytes(&bytes)
}

pub(crate) fn gb_rtc_external_identities(
    backend: &EmuBackend,
) -> Result<(TasExternalIdentity, TasExternalIdentity)> {
    let gb = backend
        .gb()
        .context("Game Boy RTC TAS backend is unavailable")?;
    let rtc = gb
        .emu
        .mbc3_rtc_state()
        .context("Game Boy RTC TAS profile requires an MBC3 timer cartridge")?;
    let ram_len = gb.emu.header().ram_size.size_bytes();
    let persisted = gb
        .emu
        .dump_battery_sram_at_time(GB_TAS_RTC_EPOCH_UNIX_SECONDS)
        .context("Game Boy RTC TAS profile requires battery-backed clock state")?;
    ensure!(
        persisted.len() == ram_len + 48,
        "Game Boy RTC TAS sidecar layout is incompatible"
    );
    let persistent_state = if ram_len == 0 {
        TasExternalIdentity::Absent
    } else {
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&persisted[..ram_len]))
    };
    let mut rtc_bytes = Vec::from(GB_RTC_STATE_CONFIGURATION);
    rtc_bytes.extend_from_slice(&rtc.internal);
    rtc_bytes.extend_from_slice(&rtc.latched);
    rtc_bytes.extend_from_slice(&rtc.subsecond_cycles.to_le_bytes());
    let rtc_state = TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&rtc_bytes));
    Ok((persistent_state, rtc_state))
}

pub(crate) fn gb_rtc_persistence_witness(backend: &EmuBackend) -> Result<GbRtcPersistenceWitness> {
    let (persistent_state, rtc_state) = gb_rtc_external_identities(backend)?;
    let complete = gb_rtc_complete_persistence_bytes(backend)?;
    let ram_len = validate_gb_rtc_runtime(backend)?;
    ensure!(
        complete.len() == ram_len + 64,
        "Game Boy RTC TAS complete sidecar layout is incompatible"
    );
    Ok(GbRtcPersistenceWitness {
        persistent_state,
        rtc_state,
        complete_byte_len: complete.len() as u64,
        complete_sha256: TasDigest::from_bytes(&complete),
    })
}

pub(crate) fn gb_rtc_complete_persistence_bytes(backend: &EmuBackend) -> Result<Vec<u8>> {
    backend
        .gb()
        .context("Game Boy RTC TAS backend is unavailable")?
        .emu
        .dump_battery_sram_with_rtc_subsecond_at_time(GB_TAS_RTC_EPOCH_UNIX_SECONDS)
        .context("Game Boy RTC TAS profile requires complete battery state")
}

pub(super) fn validate_gb_rtc_runtime(backend: &EmuBackend) -> Result<usize> {
    let provenance = backend
        .gb_tas_load_provenance()
        .context("Game Boy RTC TAS backend omitted load provenance")?;
    ensure!(
        provenance.load.rtc_time_override == Some(GB_TAS_RTC_EPOCH_UNIX_SECONDS)
            && provenance.cartridge_type.is_mbc3_with_rtc()
            && backend
                .gb()
                .and_then(|gb| gb.emu.mbc3_rtc_state())
                .is_some(),
        "Game Boy RTC TAS execution requires the fixed cycle-driven clock policy"
    );
    Ok(provenance.ram_size.size_bytes())
}

pub(super) fn gb_rtc_profile_matches(
    hardware: GbTasRtcHardware,
    identity: &crate::tas_project::TasProjectIdentity,
) -> bool {
    identity.rtc_state != TasExternalIdentity::Absent
        && [0, 8 * 1024, 32 * 1024].into_iter().any(|ram_len| {
            identity.sync_config_sha256 == gb_rtc_sync_config_sha256(hardware, ram_len, None)
                && ((ram_len == 0 && identity.persistent_state == TasExternalIdentity::Absent)
                    || (ram_len != 0
                        && matches!(
                            identity.persistent_state,
                            TasExternalIdentity::ExternalSha256(_)
                        )))
        })
}
