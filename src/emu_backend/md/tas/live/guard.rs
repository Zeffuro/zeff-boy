use super::*;
use crate::emu_backend::loader::md_tas::MAX_MD_TAS_OWNED_PAYLOAD;

pub(crate) struct MdTasLiveGuard {
    snapshot: MdTasLiveSnapshot,
    seeded: bool,
}

impl MdTasLiveGuard {
    pub(crate) fn snapshot(&self) -> &MdTasLiveSnapshot {
        &self.snapshot
    }
}

impl MdBackend {
    pub(crate) fn capture_tas_live_guard(&self, lease: &MdTasLiveLease) -> Result<MdTasLiveGuard> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &self.machine else {
            anyhow::bail!("MD TAS guard requires leased candidate")
        };
        let snapshot = MdTasLiveSnapshot {
            owner: Arc::downgrade(&state.owner),
            core: state.session.capture()?,
            proof: MdTasLiveProof::capture(state.session.core())?,
        };
        let mut graph = MdRetainedAllocations::new();
        snapshot.visit_retained(&mut graph)?;
        ensure!(
            graph.finish()?.requested_payload_bytes()
                + (size_of::<MdTasLiveGuard>() - size_of::<MdRollbackSnapshot>()) as u64
                <= MAX_MD_TAS_OWNED_PAYLOAD,
            "MD TAS guard exceeds retained payload bound"
        );
        Ok(MdTasLiveGuard {
            snapshot,
            seeded: state.seed.is_some(),
        })
    }

    pub(crate) fn restore_tas_live_guard(
        &mut self,
        lease: &MdTasLiveLease,
        guard: &MdTasLiveGuard,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        ensure!(
            guard.snapshot.owner.ptr_eq(&lease.owner),
            "foreign or stale MD TAS guard"
        );
        ensure!(
            MdTasLiveProof::capture(guard.snapshot.core.core())? == guard.snapshot.proof,
            "MD TAS guard runtime proof differs"
        );
        let MdMachine::TasLeased(state) = &mut self.machine else {
            anyhow::bail!("MD TAS guard restore requires leased candidate")
        };
        ensure!(
            !guard.seeded || state.seed.is_some(),
            "MD TAS guard lost authenticated seed"
        );
        state
            .session
            .restore(&guard.snapshot.core, &guard.snapshot.proof.config)?;
        if !guard.seeded {
            state.seed = None;
        }
        Ok(())
    }
}

#[cfg(test)]
impl MdTasLiveSnapshot {
    pub(crate) fn corrupt_tas_live_proof_for_test(&mut self) {
        self.proof.raw[0] ^= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_backend::{
        EmuBackend,
        loader::{DirectMdTasExecutionLoader, MdTasProfile},
    };

    #[test]
    fn first_live_seed_seek_source_failure_returns_preseed_runtime_queue_and_rejects_foreign_guard()
    {
        for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
            let directory = crate::test_support::test_directory("md-live-preseed-guard").unwrap();
            let path = directory.path().join("game.md");
            let mut bytes = crate::netplay::md_fixture::rom();
            std::fs::write(&path, &bytes).unwrap();
            let loader = DirectMdTasExecutionLoader::new(path.clone(), pad);
            let project = loader.create_project().unwrap();
            let mut backend = loader.load_fresh_backend().unwrap();
            let mut control = loader.load_fresh_backend().unwrap();
            backend.step_frame();
            control.step_frame();
            let EmuBackend::Md(md) = &mut backend else {
                panic!()
            };
            let original = md.core() as *const Emulator as usize;
            let lease = md.begin_tas_live(false).unwrap();
            let before = md.tas_live_proof(&lease).unwrap();
            assert!(before.queued_audio_frames() > 0);
            let guard = md.capture_tas_live_guard(&lease).unwrap();
            let EmuBackend::Md(fresh) = loader.load_fresh_backend().unwrap() else {
                panic!()
            };
            let seed = fresh
                .capture_authenticated_tas_seed(project.start_state())
                .unwrap();
            let mut foreign = fresh;
            let foreign_lease = foreign.begin_tas_live(false).unwrap();
            let foreign_before = foreign.tas_live_proof(&foreign_lease).unwrap();
            md.install_authenticated_tas_seed(&lease, seed).unwrap();
            md.advance_tas_live(&lease, [0, 0]).unwrap();
            bytes[0x3ff] ^= 1;
            std::fs::write(&path, bytes).unwrap();
            assert!(
                md.tas_live_load(&lease)
                    .unwrap()
                    .validate_source(md)
                    .is_err()
            );
            md.restore_tas_live_guard(&lease, &guard).unwrap();
            assert_eq!(md.tas_live_proof(&lease).unwrap(), before);
            assert!(md.capture_tas_live(&lease).is_err());
            assert!(
                foreign
                    .restore_tas_live_guard(&foreign_lease, &guard)
                    .is_err()
            );
            assert_eq!(
                foreign.tas_live_proof(&foreign_lease).unwrap(),
                foreign_before
            );
            foreign
                .prepare_tas_live_original_return(&foreign_lease)
                .unwrap();
            foreign
                .complete_tas_live_original(&foreign_lease, Ok(()))
                .unwrap();
            md.prepare_tas_live_original_return(&lease).unwrap();
            md.complete_tas_live_original(&lease, Ok(())).unwrap();
            assert_eq!(md.core() as *const Emulator as usize, original);
            let mut actual_pcm = Vec::new();
            let mut expected_pcm = Vec::new();
            backend.drain_audio_samples_into(&mut actual_pcm);
            control.drain_audio_samples_into(&mut expected_pcm);
            assert_eq!(actual_pcm, expected_pcm);
            for _ in 0..2 {
                backend.step_frame();
                control.step_frame();
                actual_pcm.clear();
                expected_pcm.clear();
                backend.drain_audio_samples_into(&mut actual_pcm);
                control.drain_audio_samples_into(&mut expected_pcm);
                assert!(actual_pcm.iter().any(|v| *v != 0.0));
                assert_eq!(actual_pcm, expected_pcm);
                assert_eq!(
                    backend.encode_state_bytes().unwrap(),
                    control.encode_state_bytes().unwrap()
                );
            }
        }
    }
}
