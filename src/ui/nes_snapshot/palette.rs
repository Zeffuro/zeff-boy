use std::borrow::Cow;

use crate::debug::{PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug};

pub(super) fn nes_palette_snapshot(emu: &zeff_nes_core::emulator::Emulator) -> PaletteDebugInfo {
    let palette_ram = emu.ppu_palette_ram();

    let resolve_color = |idx: usize| -> [u8; 4] {
        let nes_idx = (palette_ram[idx] as usize) & 0x3F;
        emu.palette_color_rgba(nes_idx as u8)
    };

    let build_group = |base_offset: usize, prefix: &str, title: &'static str| {
        let rows = (0..4usize)
            .map(|pal| {
                let base = base_offset + pal * 4;
                let colors: Vec<[u8; 4]> = (0..4)
                    .map(|c| {
                        if c == 0 {
                            resolve_color(0)
                        } else {
                            resolve_color(base + c)
                        }
                    })
                    .collect();
                PaletteRowDebug {
                    label: format!("{prefix} {pal}"),
                    colors,
                }
            })
            .collect();
        PaletteGroupDebug {
            title: Cow::Borrowed(title),
            rows,
        }
    };

    PaletteDebugInfo {
        groups: vec![
            build_group(0, "BG", "Background Palettes"),
            build_group(16, "OBJ", "Sprite Palettes"),
        ],
    }
}
