use zeff_md_core::{ControllerPort, Emulator};

pub(super) fn set(emu: &mut Emulator, port: ControllerPort, buttons: u8, dpad: u8) {
    emu.input_mut().set_host_pad(port, buttons, dpad);
}
