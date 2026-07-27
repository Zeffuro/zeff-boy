use std::path::PathBuf;

use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::cli::types::HeadlessOptions;

use super::super::{InputMasks, StuckReport, framebuffer_fingerprint};
use super::{input_json, input_schedule_json, screenshot_json, stuck_report_json};

pub(in crate::cli::headless_runner) fn gb_debug_state(
    emulator: &GbEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
) -> serde_json::Value {
    let serial_text = String::from_utf8_lossy(emulator.serial_output_bytes()).to_string();
    serde_json::json!({
        "system": "gb",
        "frames": frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:04X}", emulator.cpu_pc()),
        "sp": emulator.cpu_sp(),
        "sp_hex": format!("{:04X}", emulator.cpu_sp()),
        "a": emulator.cpu_a(),
        "f": emulator.cpu_f(),
        "hardware_mode": format!("{:?}", emulator.hardware_mode()),
        "cpu_state": format!("{:?}", emulator.cpu_running()),
        "ime": format!("{:?}", emulator.cpu_ime()),
        "if": emulator.if_reg(),
        "ie": emulator.ie_reg(),
        "timer": {
            "div": emulator.timer_div(),
            "tima": emulator.timer_tima(),
            "tac": emulator.timer_tac(),
        },
        "serial": {
            "bytes": emulator.serial_output_bytes().len(),
            "text": serial_text,
        },
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}
