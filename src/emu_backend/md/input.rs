use zeff_md_core::{ControllerPort, Emulator};

const BUTTON_BITS: [(u8, u8); 4] = [(6, 6), (0, 4), (1, 5), (3, 7)];
const DIRECTION_BITS: [(u8, u8); 4] = [(0, 3), (1, 2), (2, 0), (3, 1)];
const EXTRA_BITS: [(u8, u8); 4] = [(2, 3), (4, 2), (7, 1), (5, 0)];

fn project(value: u8, bits: &[(u8, u8)]) -> u8 {
    bits.iter().fold(0, |mapped, &(source, target)| {
        mapped | ((value >> source & 1) << target)
    })
}

pub(super) fn set(emu: &mut Emulator, port: ControllerPort, buttons: u8, dpad: u8) {
    emu.input_mut().set_controller_raw(
        port,
        !(project(buttons, &BUTTON_BITS) | project(dpad, &DIRECTION_BITS)),
    );
    emu.input_mut()
        .set_controller_extra_raw(port, !project(buttons, &EXTRA_BITS));
}
