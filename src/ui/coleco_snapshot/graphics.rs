use std::borrow::Cow;

use crate::debug::{
    ConsoleGraphicsData, OamDebugInfo, PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug,
    Sega8GraphicsData,
};
use zeff_coleco_core::Emulator;
use zeff_coleco_core::vdp::VdpMode;
use zeff_sega8_core::hardware::cartridge::Sega8System;
use zeff_sega8_core::hardware::vdp::{
    Mode4VdpDebugSnapshot, Tms9918Mode, Tms9918VdpDebugSnapshot, tms9918_palette_rgba,
};

const TMS_SPRITE_COUNT: usize = 32;
const TMS_SPRITE_ATTRIBUTE_BYTES: usize = 4;

pub(super) fn coleco_graphics_snapshot(
    emu: &Emulator,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
) -> ConsoleGraphicsData {
    let vdp = emu.bus().vdp();
    let snapshot = vdp.debug_snapshot();
    let mut vram = reusable_vram.unwrap_or_default();
    vram.resize(vdp.vram().len(), 0);
    vram.copy_from_slice(vdp.vram());

    let mut oam = reusable_oam.unwrap_or_default();
    copy_sprite_table(&mut oam, &vram, snapshot.sprite_attribute_table_base);

    ConsoleGraphicsData::Coleco(Box::new(Sega8GraphicsData {
        system: Sega8System::Sg1000,
        vram,
        cram: Vec::new(),
        oam,
        registers: vdp_registers(vdp.registers()),
        status: snapshot.status,
        address: snapshot.address,
        code: 0,
        v_counter: snapshot.scanline as u8,
        h_counter: 0,
        scanline: snapshot.scanline,
        scanline_cycle: u32::from(snapshot.cycles_into_line),
        line_counter: 0,
        frame_interrupt_enabled: snapshot.nmi_line,
        line_interrupt_enabled: false,
        interrupt_pending: snapshot.nmi_line,
        line_interrupt_pending: false,
        display_enabled: snapshot.display_enabled,
        tms9918_mode: format!("{:?}", snapshot.mode),
        sprite_table_base: snapshot.sprite_attribute_table_base,
        mode4: disabled_mode4_snapshot(),
        tms9918: Tms9918VdpDebugSnapshot {
            mode: tms_mode(snapshot.mode),
            name_table_base: snapshot.name_table_base,
            pattern_table_base: snapshot.pattern_table_base,
            color_table_base: snapshot.color_table_base,
            sprite_attribute_table_base: snapshot.sprite_attribute_table_base,
            sprite_pattern_table_base: snapshot.sprite_pattern_table_base,
            backdrop_color: snapshot.backdrop_color,
            text_foreground_color: snapshot.text_foreground_color,
            text_background_color: snapshot.text_background_color,
            sprite_size: if vdp.registers()[1] & 0x02 != 0 {
                16
            } else {
                8
            },
            sprite_magnified: snapshot.sprite_magnified,
        },
    }))
}

pub(super) fn coleco_oam_snapshot(emu: &Emulator) -> OamDebugInfo {
    let vdp = emu.bus().vdp();
    let snapshot = vdp.debug_snapshot();
    let rows = (0..TMS_SPRITE_COUNT)
        .map(|index| {
            let offset = snapshot.sprite_attribute_table_base + index * TMS_SPRITE_ATTRIBUTE_BYTES;
            let vram = vdp.vram();
            let y = vram[offset % vram.len()];
            let x = vram[(offset + 1) % vram.len()];
            let pattern = vram[(offset + 2) % vram.len()];
            let tag = vram[(offset + 3) % vram.len()];
            vec![
                format!("{index:02}"),
                format!("{x:03}"),
                format!("{y:02X}"),
                format!("{pattern:02X}"),
                format!("{tag:02X}"),
                format!("{:04X}", offset % vram.len()),
            ]
        })
        .collect();
    OamDebugInfo {
        headers: &["#", "X", "Y", "Pattern", "Tag", "Addr"],
        rows,
    }
}

pub(super) fn coleco_palette_snapshot() -> PaletteDebugInfo {
    PaletteDebugInfo {
        groups: vec![PaletteGroupDebug {
            title: Cow::Borrowed("TMS9918A"),
            rows: vec![PaletteRowDebug {
                label: "Fixed palette".into(),
                colors: (0..16).map(tms9918_palette_rgba).collect(),
            }],
        }],
    }
}

fn copy_sprite_table(out: &mut Vec<u8>, vram: &[u8], base: usize) {
    out.resize(TMS_SPRITE_COUNT * TMS_SPRITE_ATTRIBUTE_BYTES, 0);
    for (offset, byte) in out.iter_mut().enumerate() {
        *byte = vram[(base + offset) % vram.len()];
    }
}

fn vdp_registers(registers: &[u8; 8]) -> [u8; 16] {
    let mut output = [0; 16];
    output[..registers.len()].copy_from_slice(registers);
    output
}

fn tms_mode(mode: VdpMode) -> Tms9918Mode {
    match mode {
        VdpMode::Graphics1 => Tms9918Mode::GraphicsI,
        VdpMode::Graphics2 => Tms9918Mode::GraphicsII,
        VdpMode::Text => Tms9918Mode::Text,
        VdpMode::Multicolor => Tms9918Mode::Multicolor,
        VdpMode::Unsupported => Tms9918Mode::Invalid,
    }
}

fn disabled_mode4_snapshot() -> Mode4VdpDebugSnapshot {
    Mode4VdpDebugSnapshot {
        enabled: false,
        name_table_base: 0,
        sprite_table_base: 0,
        sprite_pattern_base: 0,
        horizontal_scroll: 0,
        vertical_scroll: 0,
        backdrop_color_index: 0,
        sprite_height: 8,
        sprite_width: 8,
        max_sprites_per_line: 4,
        horizontal_scroll_lock: false,
        vertical_scroll_lock: false,
        hide_left_column: false,
        sprite_shift_left: false,
        sprite_magnified: false,
    }
}
