use serde_json::{Value, json};

use crate::app::App;
use crate::debug::{ConsoleGraphicsData, PceVdcGraphicsData};

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
                    "oam": buffer_summary_json(&nes.oam),
                    "palette": buffer_summary_json(&nes.palette_ram),
                },
            }),
            ConsoleGraphicsData::Pce(pce) => json!({
                "ready": true,
                "system": "pce",
                "vdc1": pce_vdc_json(&pce.vdc1),
                "vdc2": pce.vdc2.as_ref().map(pce_vdc_json),
                "palette_colors": pce.palette.len(),
            }),
            ConsoleGraphicsData::Coleco(coleco) => json!({
                "ready": true,
                "system": "coleco",
                "vdp": {
                    "status": coleco.status,
                    "status_hex": format!("{:02X}", coleco.status),
                    "status_flags": {
                        "vblank": coleco.status & 0x80 != 0,
                        "sprite_overflow": coleco.status & 0x40 != 0,
                        "sprite_collision": coleco.status & 0x20 != 0,
                    },
                    "address": coleco.address,
                    "address_hex": format!("{:04X}", coleco.address),
                    "scanline": coleco.scanline,
                    "scanline_cycle": coleco.scanline_cycle,
                    "display_enabled": coleco.display_enabled,
                    "tms9918_mode": coleco.tms9918_mode,
                    "sprite_table_base": coleco.sprite_table_base,
                    "registers": coleco.registers,
                },
                "buffers": {
                    "vram": buffer_summary_json(&coleco.vram),
                    "oam": buffer_summary_json(&coleco.oam),
                },
            }),
            ConsoleGraphicsData::Sega8(sega8) => json!({
                "ready": true,
                "system": match sega8.system {
                    zeff_sega8_core::hardware::cartridge::Sega8System::MasterSystem => "sms",
                    zeff_sega8_core::hardware::cartridge::Sega8System::GameGear => "gg",
                    zeff_sega8_core::hardware::cartridge::Sega8System::Sg1000 => "sg",
                },
                "vdp": {
                    "status": sega8.status,
                    "status_hex": format!("{:02X}", sega8.status),
                    "status_flags": {
                        "vblank": sega8.status & zeff_sega8_core::hardware::constants::VDP_STATUS_VBLANK != 0,
                        "sprite_overflow": sega8.status & zeff_sega8_core::hardware::constants::VDP_STATUS_SPRITE_OVERFLOW != 0,
                        "sprite_collision": sega8.status & zeff_sega8_core::hardware::constants::VDP_STATUS_SPRITE_COLLISION != 0,
                    },
                    "address": sega8.address,
                    "address_hex": format!("{:04X}", sega8.address),
                    "code": sega8.code,
                    "v_counter": sega8.v_counter,
                    "h_counter": sega8.h_counter,
                    "scanline": sega8.scanline,
                    "scanline_cycle": sega8.scanline_cycle,
                    "line_counter": sega8.line_counter,
                    "frame_interrupt_enabled": sega8.frame_interrupt_enabled,
                    "line_interrupt_enabled": sega8.line_interrupt_enabled,
                    "interrupt_pending": sega8.interrupt_pending,
                    "line_interrupt_pending": sega8.line_interrupt_pending,
                    "display_enabled": sega8.display_enabled,
                    "tms9918_mode": sega8.tms9918_mode,
                    "sprite_table_base": sega8.sprite_table_base,
                    "mode4": {
                        "enabled": sega8.mode4.enabled,
                        "name_table_base": sega8.mode4.name_table_base,
                        "name_table_base_hex": format!("{:04X}", sega8.mode4.name_table_base),
                        "sprite_table_base": sega8.mode4.sprite_table_base,
                        "sprite_table_base_hex": format!("{:04X}", sega8.mode4.sprite_table_base),
                        "horizontal_scroll": sega8.mode4.horizontal_scroll,
                        "vertical_scroll": sega8.mode4.vertical_scroll,
                        "backdrop_color_index": sega8.mode4.backdrop_color_index,
                        "sprite_height": sega8.mode4.sprite_height,
                        "max_sprites_per_line": sega8.mode4.max_sprites_per_line,
                        "flags": {
                            "horizontal_scroll_lock": sega8.mode4.horizontal_scroll_lock,
                            "vertical_scroll_lock": sega8.mode4.vertical_scroll_lock,
                            "hide_left_column": sega8.mode4.hide_left_column,
                            "sprite_shift_left": sega8.mode4.sprite_shift_left,
                        },
                    },
                    "registers": sega8.registers,
                },
                "buffers": {
                    "vram": buffer_summary_json(&sega8.vram),
                    "cram": buffer_summary_json(&sega8.cram),
                    "oam": buffer_summary_json(&sega8.oam),
                },
            }),
        }
    }
}

fn pce_vdc_json(vdc: &PceVdcGraphicsData) -> Value {
    let mut hasher = crc32fast::Hasher::new();
    for word in &vdc.vram {
        hasher.update(&word.to_le_bytes());
    }
    json!({
        "vram_words": vdc.vram.len(),
        "vram_bytes": vdc.vram.len() * size_of::<u16>(),
        "vram_crc32": format!("{:08x}", hasher.finalize()),
        "registers": vdc.registers,
    })
}
