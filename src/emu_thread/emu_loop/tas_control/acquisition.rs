use super::*;

pub(super) struct PreparedAcquisition {
    next_lease_id: u64,
    authority: BackendAuthority,
    response: EmuResponse,
}

impl PreparedAcquisition {
    pub(super) fn retained_bytes(&self) -> usize {
        match &self.authority {
            BackendAuthority::Leased { checkpoint, .. } => {
                std::mem::size_of::<BackendAuthority>()
                    + std::mem::size_of::<TasControlCheckpoint>()
                    + checkpoint.state_bytes.capacity()
            }
            BackendAuthority::Gameplay => unreachable!(),
        }
    }
}

impl TasControl {
    pub(super) fn prepare_acquire<F>(
        &mut self,
        request_id: u64,
        profile: TasExecutionProfile,
        context: TasControlContext,
        build_witness: F,
    ) -> Result<PreparedAcquisition, TasControlAcquireRejectedReason>
    where
        F: FnOnce(
            TasExecutionProfile,
            &mut Vec<f32>,
        ) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason>,
    {
        if let Some(reason) = self.acquire_blocker(&context) {
            return Err(reason);
        }
        let lease_id = self.next_lease_id;
        let next_lease_id = lease_id
            .checked_add(1)
            .ok_or(TasControlAcquireRejectedReason::LeaseIdExhausted)?;
        let witness = build_witness(profile, &mut self.discarded_audio_samples)?;
        let checkpoint_sha256 = TasDigest::from_bytes(&witness.current_state_bytes);
        if witness.profile != profile || checkpoint_sha256 != witness.current_state_sha256 {
            return Err(TasControlAcquireRejectedReason::StateWitnessUnavailable);
        }
        let authority = BackendAuthority::Leased {
            lease_id,
            profile,
            state_format_compatibility_id: witness.state_format_compatibility_id,
            checkpoint: Box::new(TasControlCheckpoint {
                profile,
                state_bytes: witness.current_state_bytes.clone(),
                state_sha256: checkpoint_sha256,
                frame_count: witness.frame_count,
            }),
            attempted_run_id: None,
            execution_failed: false,
            candidate: None,
            intermediate_cache_proofs: Vec::new(),
        };
        Ok(PreparedAcquisition {
            next_lease_id,
            authority,
            response: EmuResponse::TasControlAcquired {
                request_id,
                lease_id,
                witness: Box::new(witness),
            },
        })
    }

    pub(super) fn accept_acquire(&mut self, prepared: PreparedAcquisition) -> EmuResponse {
        self.next_lease_id = prepared.next_lease_id;
        self.authority = prepared.authority;
        prepared.response
    }

    pub(super) fn acquire<F>(
        &mut self,
        request_id: u64,
        profile: TasExecutionProfile,
        context: TasControlContext,
        build_witness: F,
    ) -> EmuResponse
    where
        F: FnOnce(
            TasExecutionProfile,
            &mut Vec<f32>,
        ) -> Result<TasControlLeaseWitness, TasControlAcquireRejectedReason>,
    {
        match self.prepare_acquire(request_id, profile, context, build_witness) {
            Ok(prepared) => self.accept_acquire(prepared),
            Err(reason) => EmuResponse::TasControlAcquireRejected { request_id, reason },
        }
    }
}
