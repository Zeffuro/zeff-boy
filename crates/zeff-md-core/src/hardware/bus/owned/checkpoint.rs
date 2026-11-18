use super::*;

impl Bus {
    pub(crate) fn capture_board_origins(&self, storage_bytes: u64) -> Result<Option<Vec<u8>>> {
        self.owned
            .as_ref()
            .map(|owner| owner.board.capture_instruction_origins(storage_bytes))
            .transpose()
    }

    pub(crate) fn attach_board_origins(
        &mut self,
        capsule: Option<&[u8]>,
        storage_bytes: u64,
    ) -> Result<()> {
        match (self.owned.as_mut(), capsule) {
            (Some(owner), Some(capsule)) => owner
                .board
                .attach_instruction_origins(capsule, storage_bytes),
            (None, None) => Ok(()),
            _ => anyhow::bail!("MAIN origin checkpoint owner mismatch"),
        }
    }
}
