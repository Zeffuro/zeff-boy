use crate::debug::{PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug};
use zeff_nes_core::hardware::ppu::NES_PALETTE;

pub(super) fn nes_palette_snapshot(emu: &zeff_nes_core::emulator::Emulator) -> PaletteDebugInfo {
    let palette_ram = emu.ppu_palette_ram();

    let resolve_color = |idx: usize| -> [u8; 4] {
        let nes_idx = (palette_ram[idx] as usize) & 0x3F;
        let (r, g, b) = NES_PALETTE[nes_idx];
        [r, g, b, 255]
    };

    let mut groups = Vec::with_capacity(2);

    let mut bg_rows = Vec::with_capacity(4);
    for pal in 0..4usize {
        let base = pal * 4;
        let colors: Vec<[u8; 4]> = (0..4).map(|c| {
            if c == 0 {
                resolve_color(0)
            } else {
                resolve_color(base + c)
            }
        }).collect();
        bg_rows.push(PaletteRowDebug {
            label: format!("BG {pal}"),
            colors,
        });
    }
    groups.push(PaletteGroupDebug {
        title: "Background Palettes".into(),
        rows: bg_rows,
    });

    let mut obj_rows = Vec::with_capacity(4);
    for pal in 0..4usize {
        let base = 16 + pal * 4;
        let colors: Vec<[u8; 4]> = (0..4).map(|c| {
            if c == 0 {
                resolve_color(0)
            } else {
                resolve_color(base + c)
            }
        }).collect();
        obj_rows.push(PaletteRowDebug {
            label: format!("OBJ {pal}"),
            colors,
        });
    }
    groups.push(PaletteGroupDebug {
        title: "Sprite Palettes".into(),
        rows: obj_rows,
    });

    PaletteDebugInfo { groups }
}
