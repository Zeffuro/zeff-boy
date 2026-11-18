use super::*;

impl MdBackend {
    pub(crate) fn begin_tas_live(
        &mut self,
        cheats_present: bool,
    ) -> std::result::Result<MdTasLiveLease, MdTasLiveBeginError> {
        let prepared = (|| -> Result<LiveBaseline> {
            ensure!(!cheats_present, "MD TAS live excludes cheats");
            ensure!(
                self.tas_editor_owner
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_none(),
                "MD offline editor already owns this backend"
            );
            let load = self.tas_load()?;
            load.validate_fixed(self)?;
            load.validate_source(self)?;
            Ok(LiveBaseline {
                host: self.rollback_host_baseline()?,
                load: load.clone(),
                proof: MdTasLiveProof::capture(self.core())?,
            })
        })()
        .map_err(|error| MdTasLiveBeginError {
            error,
            fenced_lease: None,
        })?;
        let core_acquisition =
            MdRollbackSession::prepare_tas_acquisition(self.core(), &prepared.proof.config)
                .map_err(|error| MdTasLiveBeginError {
                    error,
                    fenced_lease: None,
                })?;
        let observation = Box::new(self.core().clone());
        let owner = Arc::new(());
        let lease = MdTasLiveLease {
            owner: Arc::downgrade(&owner),
        };
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::Normal(original) = moved else {
            self.machine = moved;
            return Err(MdTasLiveBeginError {
                error: anyhow::anyhow!("MD ownership changed during TAS acquisition"),
                fenced_lease: None,
            });
        };
        self.host_persistence_enabled = false;
        match core_acquisition.acquire_original(original) {
            Ok(session) => {
                self.machine = MdMachine::TasLeased(Box::new(LiveState {
                    session,
                    owner,
                    baseline: prepared,
                    seed: None,
                }));
                Ok(lease)
            }
            Err((original, error)) => {
                self.machine = MdMachine::TasRestoring(Box::new(LiveRestore {
                    original,
                    owner,
                    baseline: prepared,
                }));
                match self.complete_tas_live_original(&lease, Ok(())) {
                    Ok(()) => Err(MdTasLiveBeginError {
                        error,
                        fenced_lease: None,
                    }),
                    Err(restore) => Err(MdTasLiveBeginError {
                        error: error
                            .context(format!("MD failed acquisition restoration: {restore}")),
                        fenced_lease: Some(lease),
                    }),
                }
            }
        }
    }

    pub(crate) fn prepare_tas_live_promotion(
        &mut self,
        lease: &MdTasLiveLease,
        expected: &MdTasLiveProof,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &self.machine else {
            anyhow::bail!("MD TAS candidate is not leased")
        };
        let seed = state
            .seed
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MD TAS project seed is not installed"))?;
        seed.validate_for(&state.baseline.load)?;
        state.baseline.load.validate_source(self)?;
        self.verify_tas_live_original(state.session.original(), &state.baseline)?;
        ensure!(
            MdTasLiveProof::capture(state.session.core())? == *expected,
            "MD TAS promotion candidate proof differs"
        );
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::TasLeased(state) = moved else {
            unreachable!()
        };
        let LiveState {
            session,
            owner,
            baseline,
            seed,
        } = *state;
        match session.prepare_candidate_promotion(&expected.config, &expected.hashes) {
            Ok(promotion) => {
                let Some(seed) = seed else { unreachable!() };
                self.machine = MdMachine::TasCommitting(Box::new(LiveCommit {
                    promotion,
                    owner,
                    baseline,
                    seed,
                }));
                Ok(())
            }
            Err((session, error)) => {
                self.machine = MdMachine::TasLeased(Box::new(LiveState {
                    session,
                    owner,
                    baseline,
                    seed,
                }));
                Err(error)
            }
        }
    }

    pub(crate) fn finish_tas_live_promotion(
        &mut self,
        lease: &MdTasLiveLease,
        expected: &MdTasLiveProof,
        worker_finalization: Result<()>,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasCommitting(state) = &self.machine else {
            anyhow::bail!("MD TAS promotion is not staged")
        };
        let verification = (|| {
            state.seed.validate_for(&state.baseline.load)?;
            state.baseline.load.validate_source(self)?;
            self.verify_tas_live_original(state.promotion.original(), &state.baseline)?;
            ensure!(
                state.promotion.config() == &expected.config
                    && MdTasLiveProof::capture(state.promotion.candidate())? == *expected,
                "MD staged TAS candidate proof differs"
            );
            Ok(())
        })();
        if let Err(error) = worker_finalization.and(verification) {
            if let Err(restore) = self.prepare_tas_live_original_return(lease) {
                return Err(
                    error.context(format!("MD failed promotion original return: {restore}"))
                );
            }
            return Err(error);
        }
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::TasCommitting(state) = moved else {
            unreachable!()
        };
        let LiveCommit {
            promotion,
            baseline,
            ..
        } = *state;
        let candidate = promotion.finish_candidate();
        self.machine = MdMachine::Normal(candidate);
        self.host_persistence_enabled = baseline.host.publication;
        self.rotate_trace_stream();
        self.replace_sound_unwitnessed(
            "Execution owner installed a machine; earlier SOUND history is unavailable",
        );
        Ok(())
    }

    pub(crate) fn prepare_tas_live_original_return(
        &mut self,
        lease: &MdTasLiveLease,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        if let MdMachine::TasRestoring(state) = &self.machine {
            return self.verify_tas_live_original(&state.original, &state.baseline);
        }
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let state = match moved {
            MdMachine::TasLeased(state) => {
                let LiveState {
                    session,
                    owner,
                    baseline,
                    ..
                } = *state;
                LiveRestore {
                    original: session.release(),
                    owner,
                    baseline,
                }
            }
            MdMachine::TasCommitting(state) => {
                let LiveCommit {
                    promotion,
                    owner,
                    baseline,
                    ..
                } = *state;
                LiveRestore {
                    original: promotion.abort_original(),
                    owner,
                    baseline,
                }
            }
            _ => unreachable!(),
        };
        self.machine = MdMachine::TasRestoring(Box::new(state));
        let MdMachine::TasRestoring(state) = &self.machine else {
            unreachable!()
        };
        self.verify_tas_live_original(&state.original, &state.baseline)
    }

    pub(crate) fn complete_tas_live_original(
        &mut self,
        lease: &MdTasLiveLease,
        worker_original_restored: Result<()>,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasRestoring(state) = &self.machine else {
            anyhow::bail!("MD original is not in restoration fence")
        };
        let verification = self.verify_tas_live_original(&state.original, &state.baseline);
        if let Err(error) = worker_original_restored {
            return Err(match verification {
                Ok(()) => error,
                Err(restore) => {
                    error.context(format!("MD original verification failed: {restore}"))
                }
            });
        }
        verification?;
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::TasRestoring(state) = moved else {
            unreachable!()
        };
        self.machine = MdMachine::Normal(state.original);
        self.host_persistence_enabled = state.baseline.host.publication;
        self.rotate_trace_stream();
        self.replace_sound_unwitnessed(
            "Execution owner installed a machine; earlier SOUND history is unavailable",
        );
        Ok(())
    }
}
