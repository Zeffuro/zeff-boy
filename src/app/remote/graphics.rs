use serde_json::{Value, json};

use crate::app::App;
use crate::debug::ConsoleGraphicsData;

use super::json_helpers::{buffer_summary_json, gb_ppu_json};

impl App {
    pub(super) fn live_graphics_json(&mut self) -> Value {
        self.remote_debug_frames_remaining = 3;
        self.remote_graphics_frames_remaining = 3;

        let Some(gfx) = self
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.graphics_data.as_ref())
        else {
            return json!({
                "ready": false,
                "status": self.live_status_json(),
                "note": "graphics data not cached yet; retry after a frame or frame_advance while paused",
            });
        };

        match gfx {
            ConsoleGraphicsData::Gb(gb) => json!({
                "ready": true,
                "system": "gb",
                "cgb_mode": gb.cgb_mode,
                "ppu": gb_ppu_json(gb.ppu),
                "buffers": {
                    "vram": buffer_summary_json(&gb.vram),
                    "oam": buffer_summary_json(&gb.oam),
                    "bg_palette": buffer_summary_json(&gb.bg_palette_ram),
                    "obj_palette": buffer_summary_json(&gb.obj_palette_ram),
                },
            }),
            ConsoleGraphicsData::Gba(gba) => json!({
                "ready": true,
                "system": "gba",
                "ppu": {
                    "dispcnt": gba.ppu.dispcnt,
                    "bgcnt": gba.ppu.bgcnt,
                    "vcount": gba.ppu.vcount,
                    "in_vblank": gba.ppu.in_vblank,
                    "display_mode": gba.ppu.display_mode,
                    "bg_enabled": gba.ppu.bg_enabled,
                    "obj_enabled": gba.ppu.obj_enabled,
                    "obj_mapping_1d": gba.ppu.obj_mapping_1d,
                    "debug_flags": {
                        "bg": gba.ppu.debug_flags.bg,
                        "bg_layers": gba.ppu.debug_flags.bg_layers,
                        "window": gba.ppu.debug_flags.window,
                        "sprites": gba.ppu.debug_flags.sprites,
                    },
                    "non_black_pixels": gba.ppu.non_black_pixels,
                },
                "buffers": {
                    "vram": buffer_summary_json(&gba.vram),
                    "oam": buffer_summary_json(&gba.oam),
                    "palette": buffer_summary_json(&gba.palette_ram),
                },
            }),
            ConsoleGraphicsData::Nes(nes) => json!({
                "ready": true,
                "system": "nes",
                "ppu": {
                    "ctrl": nes.ctrl,
                    "mirroring": format!("{:?}", nes.mirroring),
                    "scroll_t": nes.scroll_t,
                    "fine_x": nes.fine_x,
                },
                "buffers": {
                    "chr": buffer_summary_json(&nes.chr_data),
                    "nametable": buffer_summary_json(&nes.nametable_data),
                    "palette": buffer_summary_json(&nes.palette_ram),
                },
            }),
        }
    }
}
