use serde_json::{Value, json};

use crate::app::SpeedMode;
use crate::debug::CpuDebugSnapshot;

pub(super) fn live_system_name(system: crate::emu_backend::ActiveSystem) -> &'static str {
    match system {
        crate::emu_backend::ActiveSystem::GameBoy => "gb",
        crate::emu_backend::ActiveSystem::GameBoyAdvance => "gba",
        crate::emu_backend::ActiveSystem::Nes => "nes",
        crate::emu_backend::ActiveSystem::WonderSwan => "ws",
    }
}

pub(super) fn live_speed_mode_name(speed_mode: SpeedMode) -> &'static str {
    match speed_mode {
        SpeedMode::Normal => "normal",
        SpeedMode::SlowMotion => "slow_motion",
        SpeedMode::FastForward => "fast_forward",
        SpeedMode::Uncapped => "uncapped",
    }
}

pub(super) fn cpu_debug_json(cpu: &CpuDebugSnapshot) -> Value {
    json!({
        "registers": cpu.register_lines,
        "flags": cpu.flags.iter().map(|(name, set)| json!({
            "name": name.to_string(),
            "set": set,
        })).collect::<Vec<_>>(),
        "status": cpu.status_text,
        "state": cpu.cpu_state,
        "cycles": cpu.cycles,
        "last_opcode": cpu.last_opcode_line,
        "recent_opcodes": cpu.recent_op_lines,
        "hit_breakpoint": cpu.hit_breakpoint,
        "sections": cpu.sections.iter().map(|section| json!({
            "heading": section.heading,
            "lines": section.lines,
        })).collect::<Vec<_>>(),
    })
}

pub(super) fn buffer_summary_json(bytes: &[u8]) -> Value {
    json!({
        "bytes": bytes.len(),
        "digest": format!("{:016x}", fold_bytes(bytes)),
    })
}

pub(super) fn gb_ppu_json(ppu: zeff_gb_core::debug::PpuSnapshot) -> Value {
    json!({
        "lcdc": ppu.lcdc,
        "stat": ppu.stat,
        "scy": ppu.scy,
        "scx": ppu.scx,
        "ly": ppu.ly,
        "lyc": ppu.lyc,
        "wy": ppu.wy,
        "wx": ppu.wx,
        "bgp": ppu.bgp,
        "obp0": ppu.obp0,
        "obp1": ppu.obp1,
        "cycles": ppu.cycles,
        "window_line_counter": ppu.window_line_counter,
        "window_y_triggered": ppu.window_y_triggered,
        "window_was_active_this_frame": ppu.window_was_active_this_frame,
        "window_visible_on_current_line": ppu.window_visible_on_current_line,
        "rendered_current_line": ppu.rendered_current_line,
        "draw_dots_for_line": ppu.draw_dots_for_line,
    })
}

pub(super) fn live_frame_size(
    system: crate::emu_backend::ActiveSystem,
    frame_len: usize,
) -> Option<(u32, u32)> {
    const GB_FRAME_LEN: usize = 160 * 144 * 4;
    const GBA_FRAME_LEN: usize = 240 * 160 * 4;
    const SGB_FRAME_LEN: usize = 256 * 224 * 4;
    const NES_FRAME_LEN: usize = 256 * 240 * 4;
    const WS_FRAME_LEN: usize = 224 * 144 * 4;

    match (system, frame_len) {
        (crate::emu_backend::ActiveSystem::GameBoy, GB_FRAME_LEN) => Some((160, 144)),
        (crate::emu_backend::ActiveSystem::GameBoy, SGB_FRAME_LEN) => Some((256, 224)),
        (crate::emu_backend::ActiveSystem::GameBoyAdvance, GBA_FRAME_LEN) => Some((240, 160)),
        (crate::emu_backend::ActiveSystem::Nes, NES_FRAME_LEN) => Some((256, 240)),
        (crate::emu_backend::ActiveSystem::WonderSwan, WS_FRAME_LEN) => Some((224, 144)),
        _ => None,
    }
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
