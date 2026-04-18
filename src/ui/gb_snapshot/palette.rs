use std::borrow::Cow;

use crate::debug::{PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug};
use crate::emu_thread::SnapshotRequest;
use zeff_gb_core::emulator::Emulator;
use zeff_gb_core::hardware::ppu::{
    apply_dmg_palette, cgb_palette_rgba, correct_color, dmg_palette_colors,
};

pub(super) fn gb_palette_snapshot(
    emu: &Emulator,
    show: bool,
    req: &SnapshotRequest,
) -> Option<PaletteDebugInfo> {
    if !show {
        return None;
    }
    let ppu = emu.ppu_registers();
    let cgb_mode = emu.is_cgb_mode();
    let bg_pal = emu.ppu_bg_palette_ram_snapshot();
    let obj_pal = emu.ppu_obj_palette_ram_snapshot();

    let mut groups = Vec::new();
    let preset = req.render.dmg_palette_preset;

    let dmg_row = |label: &str, val: u8| -> PaletteRowDebug {
        let colors = (0..4u8)
            .map(|cid| apply_dmg_palette(preset, val, cid))
            .collect();
        PaletteRowDebug {
            label: format!("{} ({:02X})", label, val),
            colors,
        }
    };
    groups.push(PaletteGroupDebug {
        title: Cow::Borrowed("DMG Palettes"),
        rows: vec![
            dmg_row("BGP", ppu.bgp),
            dmg_row("OBP0", ppu.obp0),
            dmg_row("OBP1", ppu.obp1),
        ],
    });

    groups.push(PaletteGroupDebug {
        title: Cow::Owned(format!("Base DMG shades ({})", preset.label())),
        rows: vec![PaletteRowDebug {
            label: String::new(),
            colors: dmg_palette_colors(preset).to_vec(),
        }],
    });

    if cgb_mode {
        let cc = req.render.color_correction;
        let ccm = req.render.color_correction_matrix;
        let cgb_group = |title: &'static str, prefix: &str, ram: &[u8; 64]| -> PaletteGroupDebug {
            let rows = (0u8..8)
                .map(|pal| {
                    let colors = (0u8..4)
                        .map(|cid| correct_color(cgb_palette_rgba(ram, pal, cid), cc, ccm))
                        .collect();
                    PaletteRowDebug {
                        label: format!("{}{}", prefix, pal),
                        colors,
                    }
                })
                .collect();
            PaletteGroupDebug {
                title: Cow::Borrowed(title),
                rows,
            }
        };
        groups.push(cgb_group("CGB BG palettes", "BG", &bg_pal));
        groups.push(cgb_group("CGB OBJ palettes", "OB", &obj_pal));
    }

    Some(PaletteDebugInfo { groups })
}
