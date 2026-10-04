use super::{ChrFetchKind, Mirroring, RomHeader};

pub(crate) trait Mapper: Send {
    fn cpu_peek(&self, addr: u16) -> u8;
    fn cpu_rom_offset(&self, _addr: u16) -> Option<usize> {
        None
    }
    fn rom_mapping_token(&self) -> u64 {
        0
    }
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.cpu_peek(addr)
    }
    fn cpu_read_open_bus(&mut self, addr: u16, _open_bus: u8) -> u8 {
        self.cpu_read(addr)
    }
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&mut self, addr: u16) -> u8;
    fn chr_read_kind(&mut self, addr: u16, _kind: ChrFetchKind) -> u8 {
        self.chr_read(addr)
    }
    fn chr_write(&mut self, addr: u16, val: u8);
    fn ppu_nametable_read(&mut self, _addr: u16, _ciram: &[u8]) -> Option<u8> {
        None
    }
    fn ppu_nametable_write(&mut self, _addr: u16, _val: u8, _ciram: &mut [u8]) -> bool {
        false
    }
    fn mirroring(&self) -> Mirroring;
    fn write_state(&self, w: &mut crate::save_state::StateWriter);
    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()>;

    // Local checkpoints retain execution transients omitted by native state.
    fn write_rollback_runtime_state(&self, _w: &mut crate::save_state::StateWriter) {}

    fn read_rollback_runtime_state(
        &mut self,
        _r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn irq_pending(&self) -> bool {
        false
    }

    fn notify_scanline(&mut self) {}

    fn uses_qualified_ppu_a12(&self) -> bool {
        false
    }

    fn notify_ppu_a12(&mut self, _high: bool, _ppu_cycle: u64) {}

    fn write_ppu_runtime_state(&self, _w: &mut crate::save_state::StateWriter) {}

    fn read_ppu_runtime_state(
        &mut self,
        _r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn clock_cpu(&mut self) {}

    fn audio_output(&self) -> f32 {
        0.0
    }

    fn load_trainer(&mut self, _bytes: &[u8], _header: &RomHeader) -> anyhow::Result<()> {
        Ok(())
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        None
    }

    fn load_battery_data(&mut self, _bytes: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn dump_persistent_data(&self) -> Option<Vec<u8>> {
        self.dump_battery_data()
    }

    fn load_persistent_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.load_battery_data(bytes)
    }

    fn write_mutable_media_state(&self, _w: &mut crate::save_state::StateWriter) {}

    fn read_mutable_media_state(
        &mut self,
        _r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset_mutable_media_to_source(&mut self) {}
}
