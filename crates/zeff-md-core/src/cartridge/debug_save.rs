use super::Cartridge;
use std::sync::Arc;

impl Cartridge {
    pub(crate) fn timed_save_owner(&self) -> bool {
        self.digest_memo.factory_timed_save
            && Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self
                .save
                .as_ref()
                .is_some_and(|save| save.ordinary_timed_header(&self.bytes))
            && self.svp.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
            && self.linear_rom.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.extended_ssf.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
    }
}
