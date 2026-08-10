use std::path::PathBuf;

use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::cpu::CpuState;

use crate::cli::types::HeadlessOptions;

use super::super::super::{AudioStats, InputMasks, StuckReport, framebuffer_fingerprint};
use super::super::{input_json, input_schedule_json, screenshot_json, stuck_report_json};
use super::objects::{gba_bg_layers_json, gba_dma_channel_json, gba_oam_json};
use super::wait::gba_last_swi_json;

#[allow(clippy::too_many_arguments)]
pub(in crate::cli::headless_runner) fn gba_debug_state(
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
        "waitcnt": read_io(0x0400_0204),
        "waitcnt_hex": format!("{:04X}", read_io(0x0400_0204)),
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
