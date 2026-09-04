use anyhow::{Context, Result, ensure};
use zeff_ws_core::hardware::cartridge::SaveKind;
use zeff_ws_core::save_state::{
    CurrentNativeWonderSwanTasRtcState, CurrentNativeWonderSwanTasStateInspection,
};

use super::WsBackend;
use crate::emu_backend::{EmuBackend, loader};
use crate::tas_project::{TasDigest, TasExternalIdentity};

const RTC_STATE_CONFIGURATION: &[u8] =
    b"zeff-ws-tas-rtc-state-v1\0policy=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00\0";
const RTC_STATE_LEN: usize = 16;
const RTC_EXTENSION_LEN: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WsRtcPersistenceWitness {
    pub(crate) save_kind: SaveKind,
    pub(crate) persistent_state: TasExternalIdentity,
    pub(crate) rtc_state: TasExternalIdentity,
    pub(crate) complete_byte_len: u64,
    pub(crate) complete_sha256: TasDigest,
}

impl WsBackend {
    pub(crate) fn tas_rtc_battery_bytes(&self) -> Option<Vec<u8>> {
        self.emu.dump_complete_rtc_persistence()
    }

    pub(crate) fn persisted_rtc_battery_receipt(
        &self,
    ) -> Result<Option<crate::save_paths::recovery_state::BatteryPublicationReceipt>> {
        if self.tas_load_provenance.is_none() || !self.emu.footer().rtc_present {
            return Ok(None);
        }
        let Some(bytes) = crate::platform::read_save_data(&crate::save_paths::sram_path_for_rom(
            self.paths.rom_path(),
        ))?
        else {
            return Ok(Some(
                crate::save_paths::recovery_state::BatteryPublicationReceipt::from_components(&[]),
            ));
        };
        rtc_battery_receipt(&self.emu, &bytes)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("persisted WonderSwan RTC sidecar layout is invalid"))
    }

    pub(crate) fn publish_tas_rtc_battery_if_unchanged(
        &mut self,
        expected: crate::save_paths::SaveTargetBaseline,
    ) -> Option<(
        String,
        crate::save_paths::SavePublicationOutcome,
        crate::save_paths::recovery_state::BatteryPublicationReceipt,
    )> {
        let bytes = self.emu.dump_complete_rtc_persistence()?;
        let receipt = rtc_battery_receipt(&self.emu, &bytes)?;
        Some(crate::save_paths::publish_battery_aggregate_if_unchanged(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            crate::save_paths::SaveRecoveryIdentity {
                system_subdir: crate::emu_backend::ActiveSystem::WonderSwan.storage_subdir(),
                media_identity: self.emu.rom_hash(),
                component: crate::save_paths::SRAM_COMPONENT,
            },
            expected,
            &bytes,
            receipt,
        ))
    }
}

pub(crate) fn ws_rtc_persistence_witness(backend: &EmuBackend) -> Result<WsRtcPersistenceWitness> {
    let inspection = loader::validate_direct_ws_tas_private_execution_runtime(backend, false)?;
    ensure!(
        inspection.rtc_present,
        "WonderSwan RTC persistence is unavailable"
    );
    let ws = backend
        .ws()
        .context("WonderSwan RTC TAS backend is unavailable")?;
    let rtc = ws
        .emu
        .dump_rtc_persistence_state()
        .context("WonderSwan RTC persistence state is unavailable")?;
    let complete = ws
        .tas_rtc_battery_bytes()
        .context("complete WonderSwan RTC persistence is unavailable")?;
    let backup = ws.emu.dump_battery_sram().unwrap_or_default();
    ensure!(
        rtc == encode_rtc_state(inspection.rtc)
            && rtc.len() == RTC_STATE_LEN
            && backup.len() == inspection.save_kind.size()
            && complete.len() == backup.len() + RTC_EXTENSION_LEN
            && complete[..backup.len()] == backup
            && complete[backup.len()..backup.len() + 8] == *b"ZBWSRTC1"
            && complete[backup.len() + 8..] == rtc,
        "WonderSwan RTC persistence layout is incompatible"
    );
    let mut validation = ws.emu.clone();
    validation.load_complete_rtc_persistence(&complete)?;
    Ok(WsRtcPersistenceWitness {
        save_kind: inspection.save_kind,
        persistent_state: ws_tas_persistent_identity(&inspection)?,
        rtc_state: ws_tas_rtc_identity(&inspection),
        complete_byte_len: complete.len() as u64,
        complete_sha256: TasDigest::from_bytes(&complete),
    })
}

pub(crate) fn ws_tas_rtc_identity(
    inspection: &CurrentNativeWonderSwanTasStateInspection,
) -> TasExternalIdentity {
    if !inspection.rtc_present {
        return TasExternalIdentity::Absent;
    }
    let mut bytes = Vec::from(RTC_STATE_CONFIGURATION);
    bytes.extend_from_slice(&encode_rtc_state(inspection.rtc));
    TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&bytes))
}

pub(crate) fn ws_tas_persistent_identity(
    inspection: &CurrentNativeWonderSwanTasStateInspection,
) -> Result<TasExternalIdentity> {
    match inspection.save_kind {
        SaveKind::None => Ok(TasExternalIdentity::Absent),
        SaveKind::Unknown(_) => {
            anyhow::bail!("direct WonderSwan TAS does not support unknown persistence")
        }
        save_kind if inspection.cartridge_save_len == save_kind.size() => Ok(
            TasExternalIdentity::ExternalSha256(TasDigest(inspection.cartridge_save_sha256)),
        ),
        _ => anyhow::bail!("direct WonderSwan TAS save size is incompatible"),
    }
}

fn encode_rtc_state(rtc: CurrentNativeWonderSwanTasRtcState) -> [u8; RTC_STATE_LEN] {
    let mut bytes = [0; RTC_STATE_LEN];
    bytes[0] = rtc.command;
    bytes[1..8].copy_from_slice(&rtc.payload);
    bytes[8] = rtc.payload_index;
    bytes[9] = rtc.payload_len;
    bytes[10] = rtc.ready_delay_reads;
    bytes[11] = u8::from(rtc.invalid_command);
    bytes[12..].copy_from_slice(&rtc.subsecond_cycles.to_le_bytes());
    bytes
}

fn rtc_battery_receipt(
    emu: &zeff_ws_core::emulator::Emulator,
    bytes: &[u8],
) -> Option<crate::save_paths::recovery_state::BatteryPublicationReceipt> {
    let backup_len = emu.footer().save_kind.size();
    if bytes.len() != backup_len + RTC_EXTENSION_LEN {
        return None;
    }
    let mut validation = emu.clone();
    validation.load_complete_rtc_persistence(bytes).ok()?;
    crate::save_paths::aggregate_battery_receipt(
        bytes,
        backup_len,
        crate::save_paths::WS_BACKUP_COMPONENT,
        crate::save_paths::WS_RTC_COMPONENT,
    )
}
