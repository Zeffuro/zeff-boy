use super::*;

impl Emulator {
    pub(super) fn write_download_state(&self, writer: &mut StateWriter) -> Result<()> {
        write_section(writer, |writer| {
            writer.write_bool(self.bus.cartridge.download.is_some());
            if let Some(board) = &self.bus.cartridge.download {
                board.write_state(writer);
            }
            Ok(())
        })
    }

    pub(super) fn read_download_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        {}
        read_section(reader, |reader| {
            ensure!(
                reader.read_bool()? == self.bus.cartridge.download.is_some(),
                "MD download RAM owner mismatch"
            );
            if let Some(board) = &mut self.bus.cartridge.download {
                board.read_state(reader)?;
            }
            Ok(())
        })
    }
}
