use crate::debug::{DebugSection, InputDebugInfo};
use zeff_ws_core::emulator::Emulator;

pub(super) fn ws_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    let keypad = emu.io_peek8(0xB5);
    InputDebugInfo {
        sections: vec![DebugSection {
            heading: "WonderSwan Keypad",
            lines: vec![format!(
                "IO B5={keypad:02X}  selected rows={}",
                selected_rows_label(keypad)
            )],
        }],
        progress_bars: Vec::new(),
    }
}

fn selected_rows_label(value: u8) -> &'static str {
    match (value & 0x10 != 0, value & 0x20 != 0, value & 0x40 != 0) {
        (false, false, false) => "none",
        (true, false, false) => "Y",
        (false, true, false) => "X",
        (false, false, true) => "Buttons",
        (true, true, false) => "Y X",
        (true, false, true) => "Y Buttons",
        (false, true, true) => "X Buttons",
        (true, true, true) => "Y X Buttons",
    }
}
