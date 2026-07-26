use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::cpu::{
    ArmInstructionClass, CpuState, DecodedInstruction, FetchedInstruction as GbaFetchedInstruction,
    ThumbInstructionClass,
};
use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::cli::types::HeadlessOptions;

use super::{AudioStats, InputMasks, StuckReport, framebuffer_fingerprint};
pub(super) fn emit_debug_state(
    opts: &HeadlessOptions,
    value: serde_json::Value,
) -> anyhow::Result<()> {
    if let Some(path) = &opts.debug_state_path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&value)?)?;
        println!("[headless] debug-state={}", path.display());
    }
    if opts.print_debug_state {
        println!("[headless-debug] {}", serde_json::to_string(&value)?);
    }
    Ok(())
}

pub(super) fn dump_gba_memory_snapshots(emulator: &GbaEmulator, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("vram.bin"), emulator.vram_snapshot())?;
    std::fs::write(dir.join("palette.bin"), emulator.palette_ram_snapshot())?;
    std::fs::write(dir.join("oam.bin"), emulator.oam_snapshot())?;
    std::fs::write(dir.join("io.bin"), emulator.io_snapshot())?;
    let (ewram, iwram) = emulator.system_ram();
    std::fs::write(dir.join("ewram.bin"), ewram)?;
    std::fs::write(dir.join("iwram.bin"), iwram)?;
    println!("[headless] gba-memory-dump={}", dir.display());
    Ok(())
}

pub(super) fn write_audio_dump_f32le(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    for &sample in samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()?;
    println!(
        "[headless] audio-dump={} format=f32le channels=2 sample_rate={} samples={}",
        path.display(),
        sample_rate,
        samples.len()
    );
    Ok(())
}

fn stuck_report_json(report: Option<&StuckReport>) -> serde_json::Value {
    match report {
        Some(report) => serde_json::json!({
            "detected": true,
            "frame": report.frame,
            "window_frames": report.window_frames,
            "unique_pcs": report.unique_pcs,
            "framebuffer_changed": report.framebuffer_changed,
            "first_pc": report.first_pc,
            "last_pc": report.last_pc,
            "classification": report.classification,
            "expected_wait": report.expected_wait,
        }),
        None => serde_json::json!({ "detected": false }),
    }
}

fn input_json(input: InputMasks) -> serde_json::Value {
    serde_json::json!({
        "buttons": input.buttons,
        "dpad": input.dpad,
        "reset": input.reset,
        "buttons_hex": format!("{:02X}", input.buttons),
        "dpad_hex": format!("{:02X}", input.dpad),
    })
}

fn input_schedule_json(opts: &HeadlessOptions) -> serde_json::Value {
    let events = opts
        .input_events
        .iter()
        .map(|event| {
            serde_json::json!({
                "start_frame": event.start_frame,
                "end_frame": event.end_frame,
                "buttons": event.buttons,
                "dpad": event.dpad,
                "reset": event.reset,
                "buttons_hex": format!("{:02X}", event.buttons),
                "dpad_hex": format!("{:02X}", event.dpad),
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "event_count": events.len(),
        "events": events,
    })
}

fn gba_last_swi_json(fetch: Option<GbaFetchedInstruction>) -> serde_json::Value {
    match gba_last_swi_function(fetch) {
        Some(function) => serde_json::json!({
            "present": true,
            "function": function,
            "function_hex": format!("{function:02X}"),
            "name": gba_swi_name(function),
            "wait_like": matches!(function, 0x02 | 0x04 | 0x05),
        }),
        None => serde_json::json!({ "present": false }),
    }
}

pub(super) fn gba_wait_classification(emulator: &GbaEmulator) -> Option<&'static str> {
    match gba_last_swi_function(emulator.last_fetch()) {
        Some(0x02) => Some("gba-swi-halt-idle"),
        Some(0x04) => Some("gba-swi-intr-wait-idle"),
        Some(0x05) => Some("gba-swi-vblank-intr-wait-idle"),
        _ => None,
    }
}

fn gba_last_swi_function(fetch: Option<GbaFetchedInstruction>) -> Option<u32> {
    let fetch = fetch?;
    match fetch.decoded {
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SoftwareInterrupt,
            ..
        } => Some(fetch.raw & 0x00FF_FFFF),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::ConditionalBranchOrSwi,
        } if fetch.raw & 0xFF00 == 0xDF00 => Some(fetch.raw & 0xFF),
        _ => None,
    }
}

fn gba_swi_name(function: u32) -> &'static str {
    match function {
        0x01 => "RegisterRamReset",
        0x02 => "Halt",
        0x04 => "IntrWait",
        0x05 => "VBlankIntrWait",
        0x06 => "Div",
        0x07 => "DivArm",
        0x08 => "Sqrt",
        0x0B => "CpuSet",
        0x0C => "CpuFastSet",
        0x10 => "BitUnPack",
        0x11 => "LZ77UnCompReadNormalWrite8bit",
        0x12 => "LZ77UnCompReadNormalWrite16bit",
        0x13 => "HuffUnComp",
        0x14 => "RLUnCompReadNormalWrite8bit",
        0x15 => "RLUnCompReadNormalWrite16bit",
        0x16 => "Diff8bitUnFilterWrite8bit",
        0x17 => "Diff8bitUnFilterWrite16bit",
        0x18 => "Diff16bitUnFilter",
        _ => "Unknown",
    }
}

fn screenshot_json(path: Option<&PathBuf>) -> serde_json::Value {
    match path {
        Some(path) => serde_json::json!({ "written": true, "path": path.display().to_string() }),
        None => serde_json::json!({ "written": false }),
    }
}

fn hex_bytes(bytes: &[u8]) -> Vec<String> {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn decode_printable_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            b'\n' | b'\r' | b'\t' => byte as char,
            0x20..=0x7E => byte as char,
            _ => '.',
        })
        .collect()
}

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

pub(super) fn gb_debug_state(
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

pub(super) fn gba_debug_state(
    emulator: &GbaEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
    audio_stats: AudioStats,
) -> serde_json::Value {
    let last_fetch = emulator.last_fetch().map(|fetch| {
        serde_json::json!({
            "pc": fetch.pc,
            "pc_hex": format!("{:08X}", fetch.pc),
            "raw": fetch.raw,
            "raw_hex": format!("{:08X}", fetch.raw),
            "instruction_set": format!("{:?}", fetch.instruction_set),
            "width_bytes": fetch.width_bytes,
            "fetch_cycles": fetch.fetch_cycles,
            "decoded": format!("{:?}", fetch.decoded),
        })
    });
    let last_swi = gba_last_swi_json(emulator.last_fetch());
    let ppu = emulator.ppu_debug_snapshot();
    let apu = emulator.apu_debug_snapshot();
    let read_io = |addr| emulator.cpu_peek16(addr);
    let irq_handler_addr = emulator.cpu_peek32(0x03FF_FFFC);
    let irq_handler_opcode = emulator.cpu_peek32(irq_handler_addr & !3);
    let display_io = serde_json::json!({
        "win0h": read_io(0x0400_0040),
        "win0h_hex": format!("{:04X}", read_io(0x0400_0040)),
        "win1h": read_io(0x0400_0042),
        "win1h_hex": format!("{:04X}", read_io(0x0400_0042)),
        "win0v": read_io(0x0400_0044),
        "win0v_hex": format!("{:04X}", read_io(0x0400_0044)),
        "win1v": read_io(0x0400_0046),
        "win1v_hex": format!("{:04X}", read_io(0x0400_0046)),
        "winin": read_io(0x0400_0048),
        "winin_hex": format!("{:04X}", read_io(0x0400_0048)),
        "winout": read_io(0x0400_004A),
        "winout_hex": format!("{:04X}", read_io(0x0400_004A)),
        "mosaic": read_io(0x0400_004C),
        "mosaic_hex": format!("{:04X}", read_io(0x0400_004C)),
        "bldcnt": read_io(0x0400_0050),
        "bldcnt_hex": format!("{:04X}", read_io(0x0400_0050)),
        "bldalpha": read_io(0x0400_0052),
        "bldalpha_hex": format!("{:04X}", read_io(0x0400_0052)),
        "bldy": read_io(0x0400_0054),
        "bldy_hex": format!("{:04X}", read_io(0x0400_0054)),
    });
    let io = serde_json::json!({
        "dispcnt": read_io(0x0400_0000),
        "dispcnt_hex": format!("{:04X}", read_io(0x0400_0000)),
        "dispstat": read_io(0x0400_0004),
        "dispstat_hex": format!("{:04X}", read_io(0x0400_0004)),
        "vcount": read_io(0x0400_0006),
        "display": display_io,
        "ie": read_io(0x0400_0200),
        "ie_hex": format!("{:04X}", read_io(0x0400_0200)),
        "if": read_io(0x0400_0202),
        "if_hex": format!("{:04X}", read_io(0x0400_0202)),
        "ime": read_io(0x0400_0208),
        "ime_hex": format!("{:04X}", read_io(0x0400_0208)),
        "keyinput": read_io(0x0400_0130),
        "keyinput_hex": format!("{:04X}", read_io(0x0400_0130)),
        "keycnt": read_io(0x0400_0132),
        "keycnt_hex": format!("{:04X}", read_io(0x0400_0132)),
        "tm0cnt_l": read_io(0x0400_0100),
        "tm0cnt_h": read_io(0x0400_0102),
        "tm1cnt_l": read_io(0x0400_0104),
        "tm1cnt_h": read_io(0x0400_0106),
        "tm2cnt_l": read_io(0x0400_0108),
        "tm2cnt_h": read_io(0x0400_010A),
        "tm3cnt_l": read_io(0x0400_010C),
        "tm3cnt_h": read_io(0x0400_010E),
        "soundcnt_l": read_io(0x0400_0080),
        "soundcnt_l_hex": format!("{:04X}", read_io(0x0400_0080)),
        "soundcnt_h": read_io(0x0400_0082),
        "soundcnt_h_hex": format!("{:04X}", read_io(0x0400_0082)),
        "soundcnt_x": read_io(0x0400_0084),
        "soundcnt_x_hex": format!("{:04X}", read_io(0x0400_0084)),
        "soundbias": read_io(0x0400_0088),
        "soundbias_hex": format!("{:04X}", read_io(0x0400_0088)),
        "irq_handler_addr": irq_handler_addr,
        "irq_handler_addr_hex": format!("{irq_handler_addr:08X}"),
        "irq_handler_opcode": irq_handler_opcode,
        "irq_handler_opcode_hex": format!("{irq_handler_opcode:08X}"),
    });
    let dma_channels = emulator.dma_channels_snapshot();
    let dma_json = serde_json::json!([
        gba_dma_channel_json(0, &read_io, dma_channels[0]),
        gba_dma_channel_json(1, &read_io, dma_channels[1]),
        gba_dma_channel_json(2, &read_io, dma_channels[2]),
        gba_dma_channel_json(3, &read_io, dma_channels[3]),
    ]);
    let apu_json = serde_json::json!({
        "sample_rate": apu.sample_rate,
        "psg_sample_rate": apu.psg_sample_rate,
        "sample_generation_enabled": apu.sample_generation_enabled,
        "sample_buffer_len": apu.sample_buffer_len,
        "drained_samples": audio_stats.sample_count,
        "drained_frames": audio_stats.frames_with_samples,
        "drained_nonzero_samples": audio_stats.nonzero_samples,
        "drained_peak_abs": audio_stats.peak_abs,
        "drained_mean_abs": audio_stats.mean_abs(),
        "output_pairs_generated": apu.output_pairs_generated,
        "direct_pairs_generated": apu.direct_pairs_generated,
        "psg_pairs_generated": apu.psg_pairs_generated,
        "fifo_len": apu.fifo_len,
        "current_sample": apu.current_sample,
        "psg_enabled": apu.psg_enabled,
        "psg_frequency": apu.psg_frequency,
        "psg_volume": apu.psg_volume,
        "channel_mutes": apu.channel_mutes,
        "direct_sound_a": {
            "enabled_right": read_io(0x0400_0082) & (1 << 8) != 0,
            "enabled_left": read_io(0x0400_0082) & (1 << 9) != 0,
            "timer": if read_io(0x0400_0082) & (1 << 10) != 0 { 1 } else { 0 },
            "volume_100_percent": read_io(0x0400_0082) & (1 << 2) != 0,
        },
        "direct_sound_b": {
            "enabled_right": read_io(0x0400_0082) & (1 << 12) != 0,
            "enabled_left": read_io(0x0400_0082) & (1 << 13) != 0,
            "timer": if read_io(0x0400_0082) & (1 << 14) != 0 { 1 } else { 0 },
            "volume_100_percent": read_io(0x0400_0082) & (1 << 3) != 0,
        },
    });
    let ppu_json = serde_json::json!({
        "dispcnt": ppu.dispcnt,
        "dispcnt_hex": format!("{:04X}", ppu.dispcnt),
        "display_mode": ppu.display_mode,
        "bgcnt": ppu.bgcnt,
        "bg_layers": gba_bg_layers_json(emulator),
        "bg_enabled": ppu.bg_enabled,
        "obj_enabled": ppu.obj_enabled,
        "obj_mapping_1d": ppu.obj_mapping_1d,
        "debug_flags": {
            "bg": ppu.debug_flags.bg,
            "bg_layers": ppu.debug_flags.bg_layers,
            "window": ppu.debug_flags.window,
            "sprites": ppu.debug_flags.sprites,
        },
        "vcount": ppu.vcount,
        "in_vblank": ppu.in_vblank,
        "non_black_pixels": ppu.non_black_pixels,
        "palette_nonzero": emulator
            .palette_ram_snapshot()
            .iter()
            .filter(|&&byte| byte != 0)
            .count(),
        "vram_nonzero": emulator
            .vram_snapshot()
            .iter()
            .filter(|&&byte| byte != 0)
            .count(),
        "oam_nonzero": emulator
            .oam_snapshot()
            .iter()
            .filter(|&&byte| byte != 0)
            .count(),
        "oam": gba_oam_json(emulator),
    });
    serde_json::json!({
        "system": "gba",
        "frames": frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:08X}", emulator.cpu_pc()),
        "visible_pc": emulator.cpu_visible_pc(),
        "visible_pc_hex": format!("{:08X}", emulator.cpu_visible_pc()),
        "cpsr": emulator.cpu_cpsr(),
        "cpsr_hex": format!("{:08X}", emulator.cpu_cpsr()),
        "thumb": emulator.cpu_thumb_state(),
        "mode": format!("{:?}", emulator.cpu_mode()),
        "cpu_state": format!("{:?}", emulator.cpu_state()),
        "halted": emulator.cpu_state() == CpuState::Halted,
        "registers": emulator.cpu_registers(),
        "suspended": emulator.is_cpu_suspended(),
        "title": &emulator.cartridge_header().title,
        "game_code": &emulator.cartridge_header().game_code,
        "backup": format!("{:?}", emulator.backup_kind()),
        "io": io,
        "dma": dma_json,
        "ppu": ppu_json,
        "apu": apu_json,
        "last_fetch": last_fetch,
        "last_swi": last_swi,
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn gba_dma_channel_json(
    channel: u32,
    read_io: &impl Fn(u32) -> u16,
    dma: zeff_gba_core::hardware::dma::DmaChannel,
) -> serde_json::Value {
    let base = 0x0400_00B0 + channel * 12;
    let source = u32::from(read_io(base)) | (u32::from(read_io(base + 2)) << 16);
    let destination = u32::from(read_io(base + 4)) | (u32::from(read_io(base + 6)) << 16);
    let count = read_io(base + 8);
    let control = read_io(base + 10);
    serde_json::json!({
        "channel": channel,
        "source": source,
        "source_hex": format!("{source:08X}"),
        "destination": destination,
        "destination_hex": format!("{destination:08X}"),
        "count": count,
        "count_hex": format!("{count:04X}"),
        "control": control,
        "control_hex": format!("{control:04X}"),
        "active_source": dma.active_source,
        "active_source_hex": format!("{:08X}", dma.active_source),
        "active_destination": dma.active_destination,
        "active_destination_hex": format!("{:08X}", dma.active_destination),
        "active_count": dma.active_count,
        "active_count_hex": format!("{:04X}", dma.active_count),
        "enabled": control & 0x8000 != 0,
        "irq": control & (1 << 14) != 0,
        "start_timing": (control >> 12) & 0x3,
        "repeat": control & (1 << 9) != 0,
        "word": control & (1 << 10) != 0,
        "destination_mode": (control >> 5) & 0x3,
        "source_mode": (control >> 7) & 0x3,
    })
}

fn gba_bg_layers_json(emulator: &GbaEmulator) -> serde_json::Value {
    serde_json::Value::Array(
        (0..4)
            .map(|bg| {
                let control = emulator.cpu_peek16(0x0400_0008 + bg * 2);
                let size = (control >> 14) & 0x3;
                let (width, height) = match size {
                    0 => (256, 256),
                    1 => (512, 256),
                    2 => (256, 512),
                    _ => (512, 512),
                };
                serde_json::json!({
                    "index": bg,
                    "enabled": emulator.cpu_peek16(0x0400_0000) & (1 << (8 + bg)) != 0,
                    "control": control,
                    "control_hex": format!("{control:04X}"),
                    "priority": control & 0x3,
                    "char_base": ((control >> 2) & 0x3) * 0x4000,
                    "screen_base": ((control >> 8) & 0x1F) * 0x800,
                    "color_256": control & (1 << 7) != 0,
                    "size": size,
                    "width": width,
                    "height": height,
                    "hofs": emulator.cpu_peek16(0x0400_0010 + bg * 4) & 0x01FF,
                    "vofs": emulator.cpu_peek16(0x0400_0012 + bg * 4) & 0x01FF,
                })
            })
            .collect(),
    )
}

fn gba_oam_json(emulator: &GbaEmulator) -> serde_json::Value {
    let oam = emulator.oam_snapshot();
    let mut active_count = 0usize;
    let mut visible_count = 0usize;
    let mut visible_objects = Vec::new();

    for obj in 0..128usize {
        let base = obj * 8;
        let attr0 = read_le16_slice(oam, base);
        let attr1 = read_le16_slice(oam, base + 2);
        let attr2 = read_le16_slice(oam, base + 4);
        let affine = attr0 & (1 << 8) != 0;
        let disabled = !affine && attr0 & (1 << 9) != 0;
        if disabled {
            continue;
        }
        let mode = (attr0 >> 10) & 0x3;
        let shape = (attr0 >> 14) & 0x3;
        let size = (attr1 >> 14) & 0x3;
        let Some((width, height)) = gba_obj_dimensions(shape, size) else {
            continue;
        };
        active_count += 1;
        let double_size = affine && attr0 & (1 << 9) != 0;
        let draw_width = if double_size { width * 2 } else { width };
        let draw_height = if double_size { height * 2 } else { height };
        let raw_y = attr0 & 0x00FF;
        let raw_x = attr1 & 0x01FF;
        let y = gba_obj_y_coord(attr0 & 0x00FF);
        let x = gba_sign_obj_coord(attr1 & 0x01FF, 512);
        let visible =
            x < 240 && x + i32::from(draw_width) > 0 && y < 160 && y + i32::from(draw_height) > 0;
        if visible {
            visible_count += 1;
        }
        if !visible {
            continue;
        }
        let affine_index = affine.then_some((attr1 >> 9) & 0x1F);
        let affine_params = affine_index.map(|index| gba_obj_affine_params(oam, index));
        visible_objects.push(serde_json::json!({
            "index": obj,
            "attr0_hex": format!("{attr0:04X}"),
            "attr1_hex": format!("{attr1:04X}"),
            "attr2_hex": format!("{attr2:04X}"),
            "raw_x": raw_x,
            "raw_y": raw_y,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
            "draw_width": draw_width,
            "draw_height": draw_height,
            "affine": affine,
            "affine_index": affine_index,
            "affine_params": affine_params.map(|(pa, pb, pc, pd)| {
                serde_json::json!({
                    "pa": pa,
                    "pb": pb,
                    "pc": pc,
                    "pd": pd,
                })
            }),
            "double_size": double_size,
            "mode": mode,
            "color_256": attr0 & (1 << 13) != 0,
            "shape": shape,
            "size": size,
            "hflip": !affine && attr1 & (1 << 12) != 0,
            "vflip": !affine && attr1 & (1 << 13) != 0,
            "tile": attr2 & 0x03FF,
            "priority": (attr2 >> 10) & 0x3,
            "palette": (attr2 >> 12) & 0xF,
        }));
    }

    serde_json::json!({
        "active_count": active_count,
        "visible_count": visible_count,
        "visible_sample": visible_objects.iter().take(24).cloned().collect::<Vec<_>>(),
        "visible_objects": visible_objects,
    })
}

fn gba_obj_dimensions(shape: u16, size: u16) -> Option<(u16, u16)> {
    match (shape, size) {
        (0, 0) => Some((8, 8)),
        (0, 1) => Some((16, 16)),
        (0, 2) => Some((32, 32)),
        (0, 3) => Some((64, 64)),
        (1, 0) => Some((16, 8)),
        (1, 1) => Some((32, 8)),
        (1, 2) => Some((32, 16)),
        (1, 3) => Some((64, 32)),
        (2, 0) => Some((8, 16)),
        (2, 1) => Some((8, 32)),
        (2, 2) => Some((16, 32)),
        (2, 3) => Some((32, 64)),
        _ => None,
    }
}

fn gba_sign_obj_coord(value: u16, range: i32) -> i32 {
    let value = i32::from(value);
    if value >= range / 2 {
        value - range
    } else {
        value
    }
}

fn gba_obj_y_coord(value: u16) -> i32 {
    let value = i32::from(value & 0x00FF);
    if value >= 160 { value - 256 } else { value }
}

fn gba_obj_affine_params(oam: &[u8], index: u16) -> (i16, i16, i16, i16) {
    let base = usize::from(index) * 0x20;
    (
        read_i16_slice(oam, base + 0x06),
        read_i16_slice(oam, base + 0x0E),
        read_i16_slice(oam, base + 0x16),
        read_i16_slice(oam, base + 0x1E),
    )
}

fn read_le16_slice(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn read_i16_slice(data: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

pub(super) fn nes_debug_state(
    emulator: &mut NesEmulator,
    frames_run: u64,
    opts: &HeadlessOptions,
    input: InputMasks,
    stuck: Option<&StuckReport>,
    screenshot: Option<&PathBuf>,
) -> serde_json::Value {
    let palette = emulator.ppu_palette_ram().to_vec();
    let nametable = emulator.ppu_nametable_ram();
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
        "frames": frames_run,
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
            "nametable_nonzero_bytes": nametable_nonzero,
            "nametable_sample": nametable_sample,
            "chr_visible_nonzero_bytes": chr_nonzero,
            "chr_visible_sample": chr_sample,
        },
        "input": input_json(input),
        "input_schedule": input_schedule_json(opts),
        "stuck": stuck_report_json(stuck),
        "screenshot": screenshot_json(screenshot),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}
