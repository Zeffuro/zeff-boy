use serde_json::{Value, json};

use crate::app::SpeedMode;
use crate::debug::CpuDebugSnapshot;
use crate::debug::common::format_addr;

pub(super) fn live_system_name(system: crate::emu_backend::ActiveSystem) -> &'static str {
    system.short_code()
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
        "breakpoints": cpu.breakpoints.iter().map(|addr| json!({
            "address": addr,
            "address_hex": format_addr(*addr),
        })).collect::<Vec<_>>(),
        "watchpoints": cpu.watchpoints.iter().map(|watch| json!({
            "address": watch.address,
            "address_hex": format_addr(watch.address),
            "type": format!("{:?}", watch.watch_type),
        })).collect::<Vec<_>>(),
        "hit_breakpoint": cpu.hit_breakpoint,
        "hit_breakpoint_hex": cpu.hit_breakpoint.map(format_addr),
        "hit_watchpoint": cpu.hit_watchpoint.as_ref().map(|hit| json!({
            "address": hit.address,
            "address_hex": format_addr(hit.address),
            "old_value": hit.old_value,
            "new_value": hit.new_value,
            "type": format!("{:?}", hit.watch_type),
        })),
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

fn fold_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::common::WatchType;
    use crate::debug::{DebugSection, WatchHitDisplay, WatchpointDisplay};

    #[test]
    fn cpu_debug_json_exposes_debug_control_state() {
        let cpu = CpuDebugSnapshot {
            register_lines: vec!["PC:1234".into()],
            flags: vec![('Z', true)],
            status_text: "State: Suspended".into(),
            cpu_state: "Suspended".into(),
            cycles: 42,
            last_opcode_line: "PC=1234 opcode=00 cycles=42".into(),
            sections: vec![DebugSection {
                heading: "Test",
                lines: vec!["ok".into()],
            }],
            mem_around_pc: [(0, 0); 32],
            recent_op_lines: vec!["1234: 00 (4 cyc)".into()],
            breakpoints: vec![0x1234],
            watchpoints: vec![WatchpointDisplay {
                address: 0xC000,
                watch_type: WatchType::ReadWrite,
            }],
            hit_breakpoint: Some(0x1234),
            hit_watchpoint: Some(WatchHitDisplay {
                address: 0xC000,
                old_value: 0x11,
                new_value: 0x22,
                watch_type: WatchType::Write,
            }),
        };

        let json = cpu_debug_json(&cpu);

        assert_eq!(json["breakpoints"][0]["address_hex"], "1234");
        assert_eq!(json["watchpoints"][0]["address_hex"], "C000");
        assert_eq!(json["watchpoints"][0]["type"], "ReadWrite");
        assert_eq!(json["hit_breakpoint_hex"], "1234");
        assert_eq!(json["hit_watchpoint"]["address_hex"], "C000");
        assert_eq!(json["hit_watchpoint"]["old_value"], 0x11);
        assert_eq!(json["hit_watchpoint"]["new_value"], 0x22);
        assert_eq!(json["hit_watchpoint"]["type"], "Write");
    }
}
