use crate::debug::{DebugSection, InputDebugInfo};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::input::ControllerPort;

pub(super) fn sega8_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    let gg_serial = emu.bus().game_gear_serial().debug_snapshot();
    InputDebugInfo {
        sections: vec![
            DebugSection {
                heading: "Controller Ports",
                lines: vec![
                    format!(
                        "P1={:02X} P2={:02X} GG_START={:02X} (active-low)",
                        emu.bus().input().read_controller(ControllerPort::One),
                        emu.bus()
                            .input()
                            .read_controller_for_bus(ControllerPort::Two, emu.console_region()),
                        emu.bus().input().read_game_gear_start(emu.console_region())
                    ),
                    format!(
                        "Host buttons map to D-pad, Button 1/Button 2, and Game Gear Start; console region={}",
                        emu.console_region().display_label()
                    ),
                ],
            },
            DebugSection {
                heading: "Game Gear Serial",
                lines: vec![
                    format!(
                        "EXT={:02X} DIR={:02X} TX={:02X} RX={:02X}",
                        gg_serial.ext_data,
                        gg_serial.ext_direction,
                        gg_serial.tx_data,
                        gg_serial.rx_data
                    ),
                    format!(
                        "CTRL={:02X} STATUS={:02X} (tx_full={} rx_ready={} error={})",
                        gg_serial.control,
                        gg_serial.status,
                        super::on_off(gg_serial.status & 0x01 != 0),
                        super::on_off(gg_serial.status & 0x02 != 0),
                        super::on_off(gg_serial.status & 0x04 != 0)
                    ),
                ],
            },
        ],
        progress_bars: Vec::new(),
    }
}
