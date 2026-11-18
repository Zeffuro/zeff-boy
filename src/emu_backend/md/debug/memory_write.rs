use super::super::MdBackend;
use zeff_emu_common::address::Address;

impl MdBackend {
    pub(in crate::emu_backend) fn memory_writes_ready(&self) -> bool {
        !self.rollback_owned() && self.suspended && !self.execution_faulted
    }

    pub(in crate::emu_backend) fn validate_memory_writes(
        &self,
        writes: &[(Address, u8)],
    ) -> Result<(), String> {
        self.require_normal().map_err(|error| error.to_string())?;
        if writes.is_empty() {
            return Ok(());
        }
        if !self.memory_writes_ready() {
            return Err("pause a healthy Mega Drive CPU before editing memory".into());
        }
        self.core().validate_debug_memory_writes(writes).map_err(|address| {
            format!("Mega Drive memory editing supports FF0000..FFFFFF RAM and A10003/05/07/09/0B/0D parallel I/O when its current state permits; rejected {address:X}")
        })
    }

    pub(in crate::emu_backend) fn apply_memory_writes(&mut self, writes: &[(Address, u8)]) {
        if self.validate_memory_writes(writes).is_ok()
            && let Ok(core) = self.same_branch_core_mut()
        {
            let _ = core.debug_write_memory(writes);
        }
    }
}
