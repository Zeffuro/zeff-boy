use std::path::PathBuf;

use crate::emu_backend::EmuBackend;
use crate::save_paths::recovery_state::{
    BatteryGenerationRecord, BatteryGenerationWitness, BatteryPublicationReceipt,
    RecoveryFreshness, RecoveryStateEnvelope, RecoveryStateIdentity, battery_generation_path,
    classify_recovery_freshness, decode_battery_generation, decode_recovery_state,
    encode_battery_generation, encode_recovery_state, reconcile_battery_generation,
    recovery_state_path,
};

pub(super) struct RecoveryCoordinator {
    system: String,
    discriminator: String,
    media_sha256: [u8; 32],
    generation_path: PathBuf,
    state_path: PathBuf,
    persisted_generation: Option<BatteryGenerationRecord>,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fail_generation_write: bool,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) struct RecoveryTestConfig {
    pub(crate) generation_path: PathBuf,
    pub(crate) state_path: PathBuf,
    pub(crate) fail_generation_write: bool,
}

pub(super) enum RecoveryCandidate {
    Missing,
    Rejected(String),
    Available {
        freshness: RecoveryFreshness,
        native_payload: Vec<u8>,
        path: PathBuf,
    },
}

#[derive(Default)]
pub(super) struct TerminalRecoveryBarrier {
    battery_committed: bool,
    generation: Option<BatteryGenerationRecord>,
}

impl TerminalRecoveryBarrier {
    pub(super) fn acknowledge_battery_commit(&mut self) {
        self.battery_committed = true;
    }

    pub(super) fn acknowledge_generation_commit(
        &mut self,
        record: BatteryGenerationRecord,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.battery_committed,
            "battery generation cannot commit before battery data"
        );
        self.generation = Some(record);
        Ok(())
    }

    pub(super) fn envelope_witness(&self) -> anyhow::Result<BatteryGenerationRecord> {
        self.generation
            .ok_or_else(|| anyhow::anyhow!("recovery envelope is blocked before generation commit"))
    }
}

pub(super) fn should_load_recovery(freshness: RecoveryFreshness, resume: bool) -> bool {
    resume && freshness == RecoveryFreshness::Fresh
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn browser_battery_flush_due(forced: bool, potentially_dirty: bool, due: bool) -> bool {
    forced || (potentially_dirty && due)
}

impl RecoveryCoordinator {
    pub(super) fn new(backend: &EmuBackend) -> Self {
        let system = backend.system().storage_subdir();
        let media_sha256 = backend.rom_hash();
        let generation_path = battery_generation_path(system, media_sha256)
            .expect("backend storage subdirectory is a safe path component");
        let state_path =
            recovery_state_path(system, backend.system().state_extension(), media_sha256)
                .expect("backend state extension is a safe path component");
        Self::new_with_paths(backend, generation_path, state_path)
    }

    fn new_with_paths(backend: &EmuBackend, generation_path: PathBuf, state_path: PathBuf) -> Self {
        let system = backend.system().storage_subdir().to_owned();
        let discriminator = backend.recovery_discriminator();
        let media_sha256 = backend.rom_hash();
        let persisted_generation = crate::platform::read_save_data(&generation_path)
            .ok()
            .flatten()
            .and_then(|bytes| decode_battery_generation(&bytes, media_sha256));
        Self {
            system,
            discriminator,
            media_sha256,
            generation_path,
            state_path,
            persisted_generation,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            fail_generation_write: false,
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn new_for_test(backend: &EmuBackend, config: RecoveryTestConfig) -> Self {
        let mut coordinator =
            Self::new_with_paths(backend, config.generation_path, config.state_path);
        coordinator.fail_generation_write = config.fail_generation_write;
        coordinator
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn prepare_generation(
        &self,
        backend: &EmuBackend,
    ) -> anyhow::Result<(BatteryGenerationRecord, Vec<u8>)> {
        let components = backend.battery_components();
        let borrowed = components
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect::<Vec<_>>();
        let receipt = BatteryPublicationReceipt::from_components(&borrowed);
        self.prepare_generation_for_receipt(&receipt)
    }

    pub(super) fn prepare_generation_for_receipt(
        &self,
        receipt: &BatteryPublicationReceipt,
    ) -> anyhow::Result<(BatteryGenerationRecord, Vec<u8>)> {
        anyhow::ensure!(
            receipt.is_consistent(),
            "battery publication receipt is inconsistent"
        );
        let record =
            reconcile_battery_generation(self.persisted_generation, receipt.component_sha256)
                .ok_or_else(|| anyhow::anyhow!("battery generation overflow"))?;
        let bytes = encode_battery_generation(self.media_sha256, record);
        Ok((record, bytes))
    }

    pub(super) fn write_generation(
        &mut self,
        backend: &EmuBackend,
    ) -> anyhow::Result<BatteryGenerationRecord> {
        let receipt = backend.battery_generation_receipt()?;
        self.write_generation_for_receipt(&receipt)
    }

    pub(super) fn write_generation_for_receipt(
        &mut self,
        receipt: &BatteryPublicationReceipt,
    ) -> anyhow::Result<BatteryGenerationRecord> {
        let (record, bytes) = self.prepare_generation_for_receipt(receipt)?;
        self.write_prepared_generation(record, bytes)
    }

    fn write_prepared_generation(
        &mut self,
        record: BatteryGenerationRecord,
        bytes: Vec<u8>,
    ) -> anyhow::Result<BatteryGenerationRecord> {
        if self.persisted_generation != Some(record) {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            anyhow::ensure!(
                !self.fail_generation_write,
                "injected battery generation write failure"
            );
            crate::platform::write_save_data(&self.generation_path, &bytes)?;
        }
        self.persisted_generation = Some(record);
        Ok(record)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn acknowledge_generation(&mut self, record: BatteryGenerationRecord) {
        self.persisted_generation = Some(record);
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn capture_generation_write(
        &self,
        backend: &EmuBackend,
    ) -> anyhow::Result<BatteryGenerationRecord> {
        let (record, bytes) = self.prepare_generation(backend)?;
        crate::platform::write_save_data(&self.generation_path, &bytes)?;
        Ok(record)
    }

    pub(super) fn write_recovery_state(
        &self,
        backend: &EmuBackend,
        record: BatteryGenerationRecord,
    ) -> anyhow::Result<PathBuf> {
        let bytes = self.encode_recovery_state(backend, record)?;
        crate::platform::write_save_data(&self.state_path, &bytes)?;
        Ok(self.state_path.clone())
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn encode_and_capture_recovery_write(
        &self,
        backend: &EmuBackend,
        record: BatteryGenerationRecord,
    ) -> anyhow::Result<PathBuf> {
        self.write_recovery_state(backend, record)
    }

    pub(super) fn inspect(&self, backend: &EmuBackend) -> RecoveryCandidate {
        let bytes = match crate::platform::read_save_data(&self.state_path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return RecoveryCandidate::Missing,
            Err(error) => return RecoveryCandidate::Rejected(error.to_string()),
        };
        let expected = RecoveryStateIdentity {
            system: &self.system,
            discriminator: &self.discriminator,
            media_sha256: self.media_sha256,
        };
        let envelope = match decode_recovery_state(&bytes, expected) {
            Ok(envelope) => envelope,
            Err(error) => return RecoveryCandidate::Rejected(error.to_string()),
        };
        let current = self.current_witness(backend);
        RecoveryCandidate::Available {
            freshness: classify_recovery_freshness(&envelope.battery, &current),
            native_payload: envelope.native_payload,
            path: self.state_path.clone(),
        }
    }

    fn encode_recovery_state(
        &self,
        backend: &EmuBackend,
        record: BatteryGenerationRecord,
    ) -> anyhow::Result<Vec<u8>> {
        let envelope = RecoveryStateEnvelope {
            system: self.system.clone(),
            discriminator: self.discriminator.clone(),
            media_sha256: self.media_sha256,
            battery: BatteryGenerationWitness::Committed {
                generation: record.generation,
                component_sha256: record.component_sha256,
            },
            native_payload: backend.encode_external_state_bytes()?,
        };
        encode_recovery_state(&envelope).map_err(Into::into)
    }

    fn current_witness(&self, backend: &EmuBackend) -> BatteryGenerationWitness {
        backend
            .battery_generation_receipt()
            .map_or(BatteryGenerationWitness::Unknown, |receipt| {
                witness_for_current_components(self.persisted_generation, receipt.component_sha256)
            })
    }
}

fn witness_for_current_components(
    persisted: Option<BatteryGenerationRecord>,
    component_sha256: [u8; 32],
) -> BatteryGenerationWitness {
    match persisted {
        Some(record) if record.component_sha256 == component_sha256 => {
            BatteryGenerationWitness::Committed {
                generation: record.generation,
                component_sha256: record.component_sha256,
            }
        }
        _ => BatteryGenerationWitness::Unknown,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn inspect_freshness_for_test(
    backend: &EmuBackend,
    config: RecoveryTestConfig,
) -> Result<RecoveryFreshness, String> {
    match RecoveryCoordinator::new_for_test(backend, config).inspect(backend) {
        RecoveryCandidate::Available { freshness, .. } => Ok(freshness),
        RecoveryCandidate::Missing => Err("recovery state is missing".to_owned()),
        RecoveryCandidate::Rejected(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_recovery_load_requires_explicit_policy_and_freshness() {
        assert!(should_load_recovery(RecoveryFreshness::Fresh, true));
        assert!(!should_load_recovery(RecoveryFreshness::Fresh, false));
        for freshness in [
            RecoveryFreshness::Stale,
            RecoveryFreshness::Unknown,
            RecoveryFreshness::Inconsistent,
        ] {
            assert!(!should_load_recovery(freshness, true));
        }
    }

    #[test]
    fn browser_periodic_flush_requires_a_dirty_witness() {
        assert!(!browser_battery_flush_due(false, false, true));
        assert!(!browser_battery_flush_due(false, true, false));
        assert!(browser_battery_flush_due(false, true, true));
        assert!(browser_battery_flush_due(true, false, false));
    }

    #[test]
    fn terminal_barrier_requires_battery_then_generation_before_envelope() {
        let record = BatteryGenerationRecord {
            generation: 9,
            component_sha256: [4; 32],
        };
        let mut barrier = TerminalRecoveryBarrier::default();
        assert!(barrier.envelope_witness().is_err());
        assert!(barrier.acknowledge_generation_commit(record).is_err());

        barrier.acknowledge_battery_commit();
        assert!(barrier.envelope_witness().is_err());
        barrier.acknowledge_generation_commit(record).unwrap();
        assert_eq!(barrier.envelope_witness().unwrap(), record);
    }

    #[test]
    fn lagging_generation_record_makes_prior_envelope_unknown() {
        let prior = BatteryGenerationRecord {
            generation: 5,
            component_sha256: [1; 32],
        };
        let captured = BatteryGenerationWitness::Committed {
            generation: prior.generation,
            component_sha256: prior.component_sha256,
        };
        let current = witness_for_current_components(Some(prior), [2; 32]);

        assert_eq!(current, BatteryGenerationWitness::Unknown);
        assert_eq!(
            classify_recovery_freshness(&captured, &current),
            RecoveryFreshness::Unknown
        );
        assert_eq!(
            reconcile_battery_generation(Some(prior), [2; 32])
                .unwrap()
                .generation,
            6
        );
    }
}
