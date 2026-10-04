use super::Cartridge;
use crate::save_state::{StateReader, StateWriter};

impl Cartridge {
    #[cfg(test)]
    pub(crate) fn rollback_mapper_variant(
        &self,
    ) -> std::mem::Discriminant<super::dispatch::MapperImpl> {
        std::mem::discriminant(&self.mapper)
    }

    pub(crate) fn has_fixed_rollback_hardware(&self) -> bool {
        self.media_slot_snapshot().is_none()
            && self.effective_mapper_label() == self.header().mapper_label()
    }

    pub(crate) fn has_portable_rollback_execution(&self) -> bool {
        self.mapper.has_portable_rollback_execution()
    }

    pub(crate) fn capture_rollback_runtime_state(&self) -> Vec<u8> {
        let mut writer = StateWriter::new();
        self.mapper.write_rollback_runtime_state(&mut writer);
        writer.into_bytes()
    }

    pub(crate) fn restore_rollback_runtime_state(&mut self, bytes: &[u8]) {
        let mut reader = StateReader::new(bytes);
        self.mapper
            .read_rollback_runtime_state(&mut reader)
            .expect("freshly captured mapper rollback runtime must decode");
        assert!(
            reader.is_exhausted(),
            "mapper rollback runtime has trailing bytes"
        );
    }
}
