use super::*;

fn lock_on_tag(board: Option<crate::cartridge::registers::LockOn>) -> u8 {
    use crate::cartridge::registers::LockOn;
    match board {
        None => 0,
        Some(LockOn::Empty) => 1,
        Some(LockOn::HalfMiB) => 2,
        Some(LockOn::MiB | LockOn::Patch) => 3,
        Some(LockOn::TwoMiB) => 4,
    }
}

impl Emulator {
    pub(super) fn write_capabilities(&self, writer: &mut StateWriter) -> Result<()> {
        write_section(writer, |writer| {
            let cart = &self.bus.cartridge;
            writer.write_u8(lock_on_tag(cart.lock_on));
            if let Some(board) = cart.lock_on {
                writer.write_bool(board.patch_enabled());
                if board.patch_enabled() {
                    board.write_patch_state(writer);
                }
            }
            writer.write_bool(cart.lock_on_save.is_some());
            if let Some(save) = &cart.lock_on_save {
                save.write_state(writer);
            }
            writer.write_bool(cart.ssf2.is_some());
            if let Some(board) = &cart.ssf2 {
                board.write_state(writer);
            }
            writer.write_bool(cart.sf002.is_some());
            if let Some(board) = &cart.sf002 {
                board.write_state(writer);
            }
            Ok(())
        })
    }

    pub(super) fn read_capabilities(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        read_section(reader, |reader| {
            let cart = &mut self.bus.cartridge;
            ensure!(
                reader.read_u8()? == lock_on_tag(cart.lock_on),
                "MD lock-on capability mismatch"
            );
            if let Some(board) = &mut cart.lock_on
                && reader.read_bool()?
            {
                board.read_patch_state(reader)?;
            }
            ensure!(
                reader.read_bool()? == cart.lock_on_save.is_some(),
                "MD inserted save capability mismatch"
            );
            if let Some(save) = &mut cart.lock_on_save {
                save.read_state(reader)?;
            }
            ensure!(
                reader.read_bool()? == cart.ssf2.is_some(),
                "MD banked ROM capability mismatch"
            );
            if let Some(board) = &mut cart.ssf2 {
                board.read_state(reader)?;
            }
            ensure!(
                reader.read_bool()? == cart.sf002.is_some(),
                "MD SF-002 capability mismatch"
            );
            if let Some(board) = &mut cart.sf002 {
                board.read_state(reader)?;
            }
            Ok(())
        })
    }
}
