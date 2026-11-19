use super::*;

impl TasControlCoordinator {
    pub(super) fn consume_acquisition_fence(
        &mut self,
        worker_generation: u64,
        response: EmuResponse,
    ) -> ResponseDisposition {
        let EmuResponse::TasControlAcquireFenced {
            request_id,
            lease_id,
            witness,
            error,
        } = response
        else {
            unreachable!()
        };
        let proof = TasControlHeldProof::from_witness(&witness);
        let expected = matches!(&self.state, TasControlState::AcquirePending {
            worker_generation: generation, request_id: expected, ..
        } if *generation == worker_generation && *expected == request_id);
        if !expected {
            return self.stale_acquired(worker_generation, lease_id, proof);
        }
        self.pending_error = Some(format!("MD TAS acquisition remains fenced: {error}"));
        self.state = TasControlState::RollbackPending {
            worker_generation,
            lease_id,
            checkpoint_sha256: proof.current_state_sha256,
            checkpoint_frame_count: proof.frame_count,
        };
        ResponseDisposition::Consumed {
            follow_up: Some(WorkerBoundCommand::rollback(worker_generation, lease_id)),
        }
    }
}
