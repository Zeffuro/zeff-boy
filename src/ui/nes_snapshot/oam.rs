use crate::debug::OamDebugInfo;

pub(super) fn nes_oam_snapshot(emu: &zeff_nes_core::emulator::Emulator) -> OamDebugInfo {
    let oam = emu.ppu_oam();
    let tall_sprites = emu.ppu_tall_sprites();

    let headers = vec![
        "#".into(),
        "X".into(),
        "Y".into(),
        "Tile".into(),
        "Attr".into(),
        "FlipH".into(),
        "FlipV".into(),
        "Pri".into(),
        "Pal".into(),
    ];

    let mut rows = Vec::with_capacity(64);
    for i in 0..64usize {
        let base = i * 4;
        let y = oam[base];
        let tile = oam[base + 1];
        let attr = oam[base + 2];
        let x = oam[base + 3];

        let flip_h = attr & 0x40 != 0;
        let flip_v = attr & 0x80 != 0;
        let priority = if attr & 0x20 != 0 { "Behind" } else { "Front" };
        let palette = attr & 0x03;

        let tile_str = if tall_sprites {
            let bank = if tile & 1 != 0 { "$1000" } else { "$0000" };
            format!("{:02X} ({})", tile, bank)
        } else {
            format!("{:02X}", tile)
        };

        rows.push(vec![
            format!("{:2}", i),
            format!("{:3}", x),
            format!("{:3}", y),
            tile_str,
            format!("{:02X}", attr),
            if flip_h { "Y" } else { "N" }.into(),
            if flip_v { "Y" } else { "N" }.into(),
            priority.into(),
            format!("{}", palette),
        ]);
    }

    OamDebugInfo { headers, rows }
}
