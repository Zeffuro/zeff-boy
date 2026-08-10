use crate::debug::{DebugSection, InputDebugInfo};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::input::ControllerPort;

pub(super) fn sega8_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    InputDebugInfo {
        sections: vec![DebugSection {
            heading: "Controller Ports",
            lines: vec![
                format!(
                    "P1={:02X} P2={:02X} (active-low)",
                    emu.bus().input().read_controller(ControllerPort::One),
                    emu.bus().input().read_controller(ControllerPort::Two)
                ),
                "Host buttons map to D-pad plus Button 1/Button 2".into(),
            ],
        }],
        progress_bars: Vec::new(),
    }
}
