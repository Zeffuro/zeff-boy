use crate::debug::{DebugSection, InputDebugInfo};
use zeff_coleco_core::Emulator;

pub(super) fn coleco_input_snapshot(emu: &Emulator) -> InputDebugInfo {
    let ports = emu.controller_ports();
    let lines = (0..2)
        .filter_map(|player| {
            let controller = ports.player(player)?;
            let value = ports.read_player(player)?;
            Some(format!(
                "P{}={:02X} up={} right={} down={} left={} L={} R={} keypad={:?}",
                player + 1,
                value,
                on_off(controller.up),
                on_off(controller.right),
                on_off(controller.down),
                on_off(controller.left),
                on_off(controller.left_button),
                on_off(controller.right_button),
                controller.keypad
            ))
        })
        .collect();
    InputDebugInfo {
        sections: vec![
            DebugSection {
                heading: "Controller Ports",
                lines,
            },
            DebugSection {
                heading: "Mux",
                lines: vec![format!("{:?} selected (active-low reads)", ports.mux())],
            },
        ],
        progress_bars: Vec::new(),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
