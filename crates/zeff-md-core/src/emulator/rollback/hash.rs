use super::{Emulator, MdRollbackConfig, MdRollbackSession, MdRollbackSnapshot};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::StateWriter;

const SUPPLEMENT_ABI: u32 = 1;
const LOGICAL_DOMAIN: &[u8] = b"Zeff-MD-rollback-logical-v1\0";
const LOCAL_DOMAIN: &[u8] = b"Zeff-MD-rollback-local-v1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdRollbackHashes {
    logical: [u8; 32],
    local: [u8; 32],
}
impl MdRollbackHashes {
    pub fn logical(&self) -> [u8; 32] {
        self.logical
    }
    pub fn local(&self) -> [u8; 32] {
        self.local
    }
}
fn section(digest: &mut Sha256, tag: u8, bytes: &[u8]) -> Result<()> {
    digest.update([tag]);
    digest.update(u64::try_from(bytes.len())?.to_le_bytes());
    digest.update(bytes);
    Ok(())
}
impl MdRollbackConfig {
    pub fn boundary_hashes(&self, core: &Emulator) -> Result<MdRollbackHashes> {
        ensure!(
            MdRollbackConfig::capture_policy(core)? == *self,
            "MD hash settings mismatch"
        );
        ensure!(
            core.concurrent_sound && core.sound_schedule.rollback_boundary_ready(),
            "MD hash requires a completed concurrent SOUND boundary"
        );
        let mut cpu = core.cpu.clone();
        let enabled = cpu.control_flow_recording();
        let events = cpu.drain_control_flow_events();
        ensure!(
            !enabled && events.events().next().is_none() && !events.overflowed(),
            "MD hash excludes a control-flow mailbox"
        );
        ensure!(
            core.bus.audio.trace_origin.is_none(),
            "MD hash excludes an audio trace origin"
        );
        let (generation, max_events) = core
            .bus
            .audio
            .trace
            .inactive_metadata()
            .ok_or_else(|| anyhow::anyhow!("MD hash excludes a retained audio trace"))?;
        let (epoch, hint) = core.bus.vdp.rollback_supplements();
        let mut logical_tail = StateWriter::new();
        logical_tail.write_u64(epoch);
        let mut local_tail = StateWriter::new();
        local_tail.write_bool(hint.is_some());
        if let Some((region, now, end)) = hint {
            local_tail.write_u8(region);
            local_tail.write_u64(now);
            local_tail.write_u64(end);
        }
        local_tail.write_u64(generation);
        local_tail.write_u64(u64::try_from(max_events)?);
        local_tail.write_bool(enabled);
        local_tail.write_u64(u64::try_from(events.events().count())?);
        local_tail.write_bool(events.overflowed());
        core.bus
            .cartridge
            .write_rollback_metadata(&mut logical_tail, &mut local_tail)?;
        let envelope = core.encode_debugger_checkpoint()?;
        let mut logical = Sha256::new();
        logical.update(LOGICAL_DOMAIN);
        logical.update(SUPPLEMENT_ABI.to_le_bytes());
        section(&mut logical, 0, &self.identity_bytes())?;
        logical.update(core.pacing_field_count().to_le_bytes());
        logical.update(core.bus.now.get().to_le_bytes());
        section(&mut logical, 1, &envelope)?;
        section(&mut logical, 2, &logical_tail.into_bytes())?;
        let logical: [u8; 32] = logical.finalize().into();
        let mut local = Sha256::new();
        local.update(LOCAL_DOMAIN);
        local.update(SUPPLEMENT_ABI.to_le_bytes());
        local.update(logical);
        section(&mut local, 3, &local_tail.into_bytes())?;
        Ok(MdRollbackHashes {
            logical,
            local: local.finalize().into(),
        })
    }
}
impl MdRollbackSession {
    pub fn hashes(&self) -> Result<MdRollbackHashes> {
        ensure!(!self.retired, "retired MD rollback session");
        self.config.boundary_hashes(&self.current)
    }
}
impl MdRollbackSnapshot {
    pub fn hashes(&self) -> Result<MdRollbackHashes> {
        self.config.boundary_hashes(&self.runtime)
    }
}

#[cfg(all(
    test,
    not(any(
        feature = "sound-shadow-observer",
        feature = "first-failure-diagnostic"
    ))
))]
mod tests;
