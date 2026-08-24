use std::path::PathBuf;

use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::cli::types::HeadlessOptions;

use super::super::{InputMasks, StuckReport, framebuffer_fingerprint};
use super::{
    decode_printable_ascii, hex_bytes, input_json, input_schedule_json, screenshot_json,
    stuck_report_json,
};

fn nes_cpu_window(emulator: &NesEmulator, start: u16, len: usize) -> Vec<u8> {
    (0..len)
        .map(|offset| emulator.cpu_peek(start.wrapping_add(offset as u16)))
        .collect()
}

fn nes_blargg_output_json(emulator: &NesEmulator) -> serde_json::Value {
    let sig = [
        emulator.cpu_peek(0x6001),
        emulator.cpu_peek(0x6002),
        emulator.cpu_peek(0x6003),
    ];
    if sig != [0xDE, 0xB0, 0x61] {
        return serde_json::json!({
            "present": false,
            "signature": hex_bytes(&sig),
        });
    }

    let status = emulator.cpu_peek(0x6000);
    let mut text = Vec::new();
    for addr in 0x6004..=0x7FFF {
        let byte = emulator.cpu_peek(addr);
        if byte == 0 {
            break;
        }
        text.push(byte);
        if text.len() >= 4096 {
            break;
        }
    }

    serde_json::json!({
        "present": true,
        "status": status,
        "status_hex": format!("{status:02X}"),
        "running": status == 0x80,
        "result": if status <= 0x7F { Some(status) } else { None },
        "text": String::from_utf8_lossy(&text).to_string(),
        "text_ascii": decode_printable_ascii(&text),
        "text_len": text.len(),
    })
}

pub(in crate::cli::headless_runner) struct NesDebugStateRequest<'a> {
    pub(in crate::cli::headless_runner) emulator: &'a mut NesEmulator,
    pub(in crate::cli::headless_runner) frames_run: u64,
    pub(in crate::cli::headless_runner) opts: &'a HeadlessOptions,
    pub(in crate::cli::headless_runner) input: InputMasks,
    pub(in crate::cli::headless_runner) input_p2: InputMasks,
    pub(in crate::cli::headless_runner) stuck: Option<&'a StuckReport>,
    pub(in crate::cli::headless_runner) screenshot: Option<&'a PathBuf>,
}

pub(in crate::cli::headless_runner) fn nes_debug_state(
    request: NesDebugStateRequest<'_>,
) -> serde_json::Value {
    let emulator = request.emulator;
    let palette = emulator.ppu_palette_ram().to_vec();
    let oam = emulator.ppu_oam().to_vec();
    let active_oam = oam
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, sprite)| sprite[0] < 0xEF)
        .map(|(index, sprite)| {
            serde_json::json!({
                "index": index,
                "y": sprite[0],
                "tile": sprite[1],
                "tile_hex": format!("{:02X}", sprite[1]),
                "attr": sprite[2],
                "attr_hex": format!("{:02X}", sprite[2]),
                "x": sprite[3],
            })
        })
        .collect::<Vec<_>>();
    let nametable = emulator.ppu_nametable_ram().to_vec();
    let nametable_nonzero = nametable.iter().filter(|&&byte| byte != 0).count();
    let nametable_sample = nametable.iter().take(128).copied().collect::<Vec<_>>();
    let chr = emulator.chr_ram_snapshot();
    let chr_nonzero = chr.iter().filter(|&&byte| byte != 0).count();
    let chr_sample = chr.iter().take(128).copied().collect::<Vec<_>>();
    let internal_ram_sample = emulator
        .system_ram()
        .iter()
        .take(256)
        .copied()
        .collect::<Vec<_>>();
    let prg_ram_sample = nes_cpu_window(emulator, 0x6000, 256);
    let blargg = nes_blargg_output_json(emulator);

    serde_json::json!({
        "system": "nes",
        "frames": request.frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:04X}", emulator.cpu_pc()),
        "suspended": emulator.is_cpu_suspended(),
        "cpu": {
            "a": emulator.cpu_a(),
            "a_hex": format!("{:02X}", emulator.cpu_a()),
            "x": emulator.cpu_x(),
            "x_hex": format!("{:02X}", emulator.cpu_x()),
            "y": emulator.cpu_y(),
            "y_hex": format!("{:02X}", emulator.cpu_y()),
            "sp": emulator.cpu_sp(),
            "sp_hex": format!("{:02X}", emulator.cpu_sp()),
            "status": emulator.cpu_status(),
            "status_hex": format!("{:02X}", emulator.cpu_status()),
            "last_opcode": emulator.cpu_last_opcode(),
            "last_opcode_hex": format!("{:02X}", emulator.cpu_last_opcode()),
            "last_opcode_pc": emulator.last_opcode_pc(),
            "last_opcode_pc_hex": format!("{:04X}", emulator.last_opcode_pc()),
            "last_step_cycles": emulator.cpu_last_step_cycles(),
            "nmi_pending": emulator.cpu_nmi_pending(),
            "irq_line": emulator.cpu_irq_line(),
            "nmi_count": emulator.cpu_nmi_count(),
            "irq_count": emulator.cpu_irq_count(),
            "vectors": {
                "nmi": {
                    "lo": emulator.cpu_peek(0xFFFA),
                    "hi": emulator.cpu_peek(0xFFFB),
                    "addr": (emulator.cpu_peek(0xFFFA) as u16)
                        | ((emulator.cpu_peek(0xFFFB) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFA) as u16)
                            | ((emulator.cpu_peek(0xFFFB) as u16) << 8)
                    ),
                },
                "reset": {
                    "lo": emulator.cpu_peek(0xFFFC),
                    "hi": emulator.cpu_peek(0xFFFD),
                    "addr": (emulator.cpu_peek(0xFFFC) as u16)
                        | ((emulator.cpu_peek(0xFFFD) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFC) as u16)
                            | ((emulator.cpu_peek(0xFFFD) as u16) << 8)
                    ),
                },
                "irq": {
                    "lo": emulator.cpu_peek(0xFFFE),
                    "hi": emulator.cpu_peek(0xFFFF),
                    "addr": (emulator.cpu_peek(0xFFFE) as u16)
                        | ((emulator.cpu_peek(0xFFFF) as u16) << 8),
                    "addr_hex": format!(
                        "{:04X}",
                        (emulator.cpu_peek(0xFFFE) as u16)
                            | ((emulator.cpu_peek(0xFFFF) as u16) << 8)
                    ),
                },
            },
        },
        "memory": {
            "internal_ram_sample": internal_ram_sample,
            "internal_ram_sample_hex": hex_bytes(&internal_ram_sample),
            "cpu_6000_sample": prg_ram_sample,
            "cpu_6000_sample_hex": hex_bytes(&prg_ram_sample),
            "blargg": blargg,
        },
        "mapper": emulator.cartridge_header().mapper_label(),
        "mapper_effective": emulator.cartridge_effective_mapper_label(),
        "battery": emulator.has_battery(),
        "ppu": {
            "ctrl": emulator.ppu_ctrl(),
            "ctrl_hex": format!("{:02X}", emulator.ppu_ctrl()),
            "mask": emulator.ppu_mask(),
            "mask_hex": format!("{:02X}", emulator.ppu_mask()),
            "status": emulator.ppu_status(),
            "status_hex": format!("{:02X}", emulator.ppu_status()),
            "scanline": emulator.ppu_scanline(),
            "dot": emulator.ppu_dot(),
            "frame_count": emulator.ppu_frame_count(),
            "in_vblank": emulator.ppu_in_vblank(),
            "frame_ready": emulator.ppu_frame_ready(),
            "scroll_v": emulator.ppu_scroll_v(),
            "scroll_t": emulator.ppu_scroll_t(),
            "fine_x": emulator.ppu_fine_x(),
            "tall_sprites": emulator.ppu_tall_sprites(),
            "palette_ram": palette,
            "oam": oam,
            "active_oam": active_oam,
            "nametable_nonzero_bytes": nametable_nonzero,
            "nametable_sample": nametable_sample,
            "nametable": nametable,
            "chr_visible_nonzero_bytes": chr_nonzero,
            "chr_visible_sample": chr_sample,
            "chr_visible": chr,
        },
        "input": input_json(request.input),
        "input_p2": input_json(request.input_p2),
        "input_schedule": input_schedule_json(request.opts),
        "stuck": stuck_report_json(request.stuck),
        "screenshot": screenshot_json(request.screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}
