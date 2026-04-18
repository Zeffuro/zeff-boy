use crate::debug::OamDebugInfo;
use zeff_gb_core::emulator::Emulator;

pub(super) fn gb_oam_snapshot(
    emu: &Emulator,
    show: bool,
    reusable_oam: Option<Vec<u8>>,
) -> (Option<OamDebugInfo>, Option<Vec<u8>>) {
    if !show {
        return (
            None,
            reusable_oam.map(|mut v| {
                v.clear();
                v
            }),
        );
    }
    use zeff_gb_core::hardware::ppu::SpriteEntry;
    let src = emu.oam();
    let mut buf = reusable_oam.unwrap_or_default();
    buf.resize(src.len(), 0);
    buf.copy_from_slice(src);

    let headers: &'static [&'static str] =
        &["#", "X", "Y", "Tile", "Flags", "FlipX", "FlipY", "Prio", "Pal", "CGB Pal", "VRAM"];
    let mut rows = Vec::with_capacity(40);
    for i in 0..40usize {
        let sprite = SpriteEntry::from_oam(&buf, i);
        rows.push(vec![
            format!("{:02}", i),
            format!("{:4}", sprite.x),
            format!("{:4}", sprite.y),
            format!("{:02X}", sprite.tile),
            format!("{:02X}", sprite.flags),
            (if sprite.flip_x() { "Y" } else { "N" }).to_string(),
            (if sprite.flip_y() { "Y" } else { "N" }).to_string(),
            (if sprite.bg_priority() { "BG" } else { "FG" }).to_string(),
            format!("{}", sprite.palette_number()),
            format!("{}", sprite.cgb_obj_palette_index()),
            format!("{}", sprite.cgb_vram_bank()),
        ]);
    }
    (Some(OamDebugInfo { headers, rows }), Some(buf))
}
