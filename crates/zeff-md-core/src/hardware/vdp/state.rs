use super::{ControlCommand, constants::*};
use anyhow::{Result, ensure};
pub(super) use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(super) fn write_command(w: &mut StateWriter, command: ControlCommand) {
    w.write_u32(command.address);
    w.write_u8(command.code);
}

pub(super) fn read_command(r: &mut StateReader<'_>) -> Result<ControlCommand> {
    let command = ControlCommand {
        address: r.read_u32()?,
        code: r.read_u8()?,
    };
    validate_command(command)?;
    Ok(command)
}

pub(super) fn validate_command(command: ControlCommand) -> Result<()> {
    ensure!(
        command.address <= COMMAND_ADDRESS_MASK && command.code & !COMMAND_CODE_MASK == 0,
        "invalid VDP command"
    );
    Ok(())
}

pub(super) fn write_optional<T: Copy>(
    w: &mut StateWriter,
    value: Option<T>,
    write: impl FnOnce(&mut StateWriter, T),
) {
    w.write_bool(value.is_some());
    if let Some(value) = value {
        write(w, value);
    }
}

pub(super) fn read_optional<'a, T>(
    r: &mut StateReader<'a>,
    read: impl FnOnce(&mut StateReader<'a>) -> Result<T>,
) -> Result<Option<T>> {
    if r.read_bool()? {
        Ok(Some(read(r)?))
    } else {
        Ok(None)
    }
}
