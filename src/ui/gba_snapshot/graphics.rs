pub(super) fn gba_graphics_data(
    emu: &zeff_gba_core::emulator::Emulator,
    vram_buffer: Option<Vec<u8>>,
) -> crate::debug::GbaGraphicsData {
    let mut vram = vram_buffer.unwrap_or_default();
    let src = emu.vram_snapshot();
    vram.resize(src.len(), 0);
    vram.copy_from_slice(src);

    crate::debug::GbaGraphicsData {
        vram,
        palette_ram: emu.palette_ram_snapshot().to_vec(),
        oam: emu.oam_snapshot().to_vec(),
        ppu: emu.ppu_debug_snapshot(),
    }
}

pub(super) fn gba_oam_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
) -> crate::debug::OamDebugInfo {
    let oam = emu.oam_snapshot();
    let rows = (0..128usize)
        .filter_map(|i| {
            let base = i * 8;
            let attr0 = read_le16(oam, base);
            let attr1 = read_le16(oam, base + 2);
            let attr2 = read_le16(oam, base + 4);
            let disabled = attr0 & 0x0300 == 0x0200;
            if disabled && i >= 32 {
                return None;
            }
            Some(vec![
                format!("{i:03}"),
                format!("{attr0:04X}"),
                format!("{attr1:04X}"),
                format!("{attr2:04X}"),
                format!("x={} y={}", attr1 & 0x01FF, attr0 & 0x00FF),
                format!("tile={:03X}", attr2 & 0x03FF),
                format!("pal={}", (attr2 >> 12) & 0xF),
                if disabled {
                    "disabled".into()
                } else {
                    "on".into()
                },
            ])
        })
        .collect();
    crate::debug::OamDebugInfo {
        headers: &[
            "#", "Attr0", "Attr1", "Attr2", "Pos", "Tile", "Pal", "State",
        ],
        rows,
    }
}

pub(super) fn gba_palette_snapshot(
    emu: &zeff_gba_core::emulator::Emulator,
) -> crate::debug::PaletteDebugInfo {
    let palette_ram = emu.palette_ram_snapshot();
    let groups = [("BG palettes", 0usize), ("OBJ palettes", 0x100usize)]
        .into_iter()
        .map(|(title, base)| crate::debug::PaletteGroupDebug {
            title: title.into(),
            rows: (0..16usize)
                .map(|pal| crate::debug::PaletteRowDebug {
                    label: format!("{pal:02}"),
                    colors: (0..16usize)
                        .map(|color| gba_palette_rgba(palette_ram, base + pal * 16 + color))
                        .collect(),
                })
                .collect(),
        })
        .collect();
    crate::debug::PaletteDebugInfo { groups }
}

fn read_le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn gba_palette_rgba(palette_ram: &[u8], index: usize) -> [u8; 4] {
    let color = read_le16(palette_ram, index * 2);
    let r = (color & 0x1F) as u8;
    let g = ((color >> 5) & 0x1F) as u8;
    let b = ((color >> 10) & 0x1F) as u8;
    [expand5(r), expand5(g), expand5(b), 255]
}

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}
