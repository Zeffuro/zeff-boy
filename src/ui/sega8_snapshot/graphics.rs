use crate::debug::{
    ConsoleGraphicsData, OamDebugInfo, PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug,
    Sega8GraphicsData,
};
use std::borrow::Cow;
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::Sega8System;
use zeff_sega8_core::hardware::constants::{
    GG_COLOR_CHANNEL_SCALE_4BIT, MODE4_SPRITE_TABLE_BYTES, MODE4_SPRITE_TERMINATOR_Y,
    MODE4_SPRITE_X_TILE_TABLE_OFFSET, SMS_COLOR_CHANNEL_SCALE_2BIT, SMS_CRAM_SIZE,
    SMS_GG_COLOR_INDEX_MASK, SMS_VRAM_SIZE,
};

const MODE4_SPRITE_COUNT: usize = 64;
const TMS_SPRITE_COUNT: usize = 32;
const TMS_SPRITE_ATTRIBUTE_BYTES: usize = 4;
const PALETTE_COLORS_PER_ROW: usize = 16;
const PALETTE_ROW_COUNT: usize = 2;

pub(super) fn sega8_graphics_snapshot(
    emu: &Emulator,
    reusable_vram: Option<Vec<u8>>,
    reusable_oam: Option<Vec<u8>>,
) -> ConsoleGraphicsData {
    let vdp = emu.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    let tms9918 = vdp.tms9918_debug_snapshot();
    let mut vram = reusable_vram.unwrap_or_default();
    vram.resize(SMS_VRAM_SIZE, 0);
    vram.copy_from_slice(vdp.vram());

    let mut oam = reusable_oam.unwrap_or_default();
    if mode4.enabled {
        copy_sprite_table(&mut oam, vdp.vram(), mode4.sprite_table_base);
    } else {
        copy_tms_sprite_table(&mut oam, vdp.vram(), tms9918.sprite_attribute_table_base);
    }

    ConsoleGraphicsData::Sega8(Box::new(Sega8GraphicsData {
        system: emu.system(),
        vram,
        cram: vdp.cram().to_vec(),
        oam,
        registers: *vdp.registers(),
        status: vdp.status(),
        address: vdp.address(),
        code: vdp.code(),
        v_counter: vdp.v_counter(),
        h_counter: vdp.h_counter(),
        scanline: vdp.scanline(),
        scanline_cycle: vdp.scanline_cycle(),
        line_counter: vdp.line_counter(),
        frame_interrupt_enabled: vdp.frame_interrupt_enabled(),
        line_interrupt_enabled: vdp.line_interrupt_enabled(),
        interrupt_pending: vdp.interrupt_pending(),
        line_interrupt_pending: vdp.line_interrupt_pending(),
        display_enabled: vdp.display_enabled(),
        tms9918_mode: format!("{:?}", vdp.tms9918_mode()),
        sprite_table_base: if mode4.enabled {
            mode4.sprite_table_base
        } else {
            tms9918.sprite_attribute_table_base
        },
        mode4,
        tms9918,
    }))
}

pub(super) fn sega8_oam_snapshot(emu: &Emulator) -> OamDebugInfo {
    let vdp = emu.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    let tms9918 = vdp.tms9918_debug_snapshot();
    if !mode4.enabled {
        return tms_oam_snapshot(vdp.vram(), tms9918.sprite_attribute_table_base);
    }
    let sprite_table_base = mode4.sprite_table_base;
    let vram = vdp.vram();
    let rows = (0..MODE4_SPRITE_COUNT)
        .filter_map(|index| {
            let y = vram[(sprite_table_base + index) % vram.len()];
            if y == MODE4_SPRITE_TERMINATOR_Y && index > 0 {
                return None;
            }
            let xt_base = sprite_table_base + MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
            let x = vram[xt_base % vram.len()];
            let tile = vram[(xt_base + 1) % vram.len()];
            Some(vec![
                format!("{index:02}"),
                format!("{x:03}"),
                format!("{y:03}"),
                format!("{tile:02X}"),
                format!("{:04X}", xt_base % vram.len()),
            ])
        })
        .collect();
    OamDebugInfo {
        headers: &["#", "X", "Y", "Tile", "XT Addr"],
        rows,
    }
}

fn tms_oam_snapshot(vram: &[u8], sprite_table_base: usize) -> OamDebugInfo {
    let rows = (0..TMS_SPRITE_COUNT)
        .map(|index| {
            let offset = sprite_table_base + index * TMS_SPRITE_ATTRIBUTE_BYTES;
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

pub(super) fn sega8_palette_snapshot(emu: &Emulator) -> PaletteDebugInfo {
    let rows = (0..PALETTE_ROW_COUNT)
        .map(|row| PaletteRowDebug {
            label: if row == 0 { "BG" } else { "OBJ" }.into(),
            colors: (0..PALETTE_COLORS_PER_ROW)
                .map(|col| {
                    sega8_palette_rgba(
                        emu.system(),
                        emu.bus().vdp().cram(),
                        row * PALETTE_COLORS_PER_ROW + col,
                    )
                })
                .collect(),
        })
        .collect();
    PaletteDebugInfo {
        groups: vec![PaletteGroupDebug {
            title: Cow::Borrowed("CRAM"),
            rows,
        }],
    }
}

fn copy_sprite_table(buf: &mut Vec<u8>, vram: &[u8], sprite_table_base: usize) {
    buf.resize(MODE4_SPRITE_TABLE_BYTES, 0);
    for (offset, byte) in buf.iter_mut().enumerate() {
        *byte = vram[(sprite_table_base + offset) % vram.len()];
    }
}

fn copy_tms_sprite_table(buf: &mut Vec<u8>, vram: &[u8], sprite_table_base: usize) {
    buf.resize(TMS_SPRITE_COUNT * TMS_SPRITE_ATTRIBUTE_BYTES, 0);
    for (offset, byte) in buf.iter_mut().enumerate() {
        *byte = vram[(sprite_table_base + offset) % vram.len()];
    }
}

fn sega8_palette_rgba(
    system: Sega8System,
    cram: &[u8; SMS_CRAM_SIZE],
    color_index: usize,
) -> [u8; 4] {
    match system {
        Sega8System::GameGear => {
            let base = (color_index & SMS_GG_COLOR_INDEX_MASK) * 2;
            let raw = u16::from_le_bytes([cram[base], cram[(base + 1) % cram.len()]]);
            [
                ((raw & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                (((raw >> 4) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                (((raw >> 8) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
                0xFF,
            ]
        }
        Sega8System::MasterSystem | Sega8System::Sg1000 => {
            let raw = cram[color_index & SMS_GG_COLOR_INDEX_MASK];
            [
                (raw & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                ((raw >> 2) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                ((raw >> 4) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
                0xFF,
            ]
        }
    }
}
