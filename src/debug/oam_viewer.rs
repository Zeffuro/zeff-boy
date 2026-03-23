use crate::hardware::ppu::SpriteEntry;


pub(super) fn draw_oam_viewer_content(ui: &mut egui::Ui, oam: &[u8]) {
    egui::Grid::new("oam_grid").striped(true).show(ui, |ui| {
        ui.strong("#");
        ui.strong("X");
        ui.strong("Y");
        ui.strong("Tile");
        ui.strong("Flags");
        ui.strong("FlipX");
        ui.strong("FlipY");
        ui.strong("Prio");
        ui.strong("Pal");
        ui.strong("CGB Pal");
        ui.strong("VRAM");
        ui.end_row();

        for i in 0..40usize {
            let sprite = SpriteEntry::from_oam(oam, i);
            ui.monospace(format!("{:02}", i));
            ui.monospace(format!("{:4}", sprite.x));
            ui.monospace(format!("{:4}", sprite.y));
            ui.monospace(format!("{:02X}", sprite.tile));
            ui.monospace(format!("{:02X}", sprite.flags));
            ui.monospace(if sprite.flip_x() { "Y" } else { "N" });
            ui.monospace(if sprite.flip_y() { "Y" } else { "N" });
            ui.monospace(if sprite.bg_priority() { "BG" } else { "FG" });
            ui.monospace(format!("{}", sprite.palette_number()));
            ui.monospace(format!("{}", sprite.cgb_obj_palette_index()));
            ui.monospace(format!("{}", sprite.cgb_vram_bank()));
            ui.end_row();
        }
    });
}
