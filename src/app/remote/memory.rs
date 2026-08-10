use serde_json::{Value, json};

use crate::app::App;
use crate::debug::ConsoleGraphicsData;

impl App {
    pub(super) fn live_memory_json(&mut self, space: &str, start: u32, length: usize) -> Value {
        let space = normalized_space_name(space);
        if space == "cpu" || space == "memory" || space == "ram" {
            self.remote_memory_view_start = Some(start);
            self.remote_memory_frames_remaining = 3;
            return self.live_cpu_memory_json(start, length.min(256));
        }

        if space != "framebuffer" {
            self.remote_graphics_frames_remaining = 3;
        }
        self.live_buffer_memory_json(&space, start as usize, length)
    }

    fn live_cpu_memory_json(&self, start: u32, length: usize) -> Value {
        let Some(page) = self
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.memory_page.as_ref())
        else {
            return memory_not_ready_json("cpu", start, length, "CPU memory page not cached yet");
        };

        let page_start = page.first().map(|(addr, _)| *addr);
        if page_start != Some(start) {
            return json!({
                "ready": false,
                "space": "cpu",
                "start": start,
                "requested_length": length,
                "cached_start": page_start,
                "note": "requested CPU memory page is not cached yet",
            });
        }

        let bytes = page
            .iter()
            .take(length)
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        memory_ready_json("cpu", start, page.len(), &bytes)
    }

    fn live_buffer_memory_json(&self, space: &str, start: usize, length: usize) -> Value {
        if space == "framebuffer" {
            let Some(frame) = self
                .latest_frame
                .as_ref()
                .or(self.last_displayed_frame.as_ref())
            else {
                return memory_not_ready_json(
                    space,
                    start as u32,
                    length,
                    "framebuffer not available yet",
                );
            };
            return slice_memory_json(space, start, length, frame);
        }

        let Some(gfx) = self
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.graphics_data.as_ref())
        else {
            return memory_not_ready_json(
                space,
                start as u32,
                length,
                "graphics data not cached yet",
            );
        };

        match gfx {
            ConsoleGraphicsData::Gb(gb) => match space {
                "vram" => slice_memory_json(space, start, length, &gb.vram),
                "oam" => slice_memory_json(space, start, length, &gb.oam),
                "palette" | "bgpalette" | "bgpaletteram" => {
                    slice_memory_json(space, start, length, &gb.bg_palette_ram)
                }
                "objpalette" | "objpaletteram" | "spritepalette" => {
                    slice_memory_json(space, start, length, &gb.obj_palette_ram)
                }
                _ => memory_not_ready_json(
                    space,
                    start as u32,
                    length,
                    "unsupported GB memory space",
                ),
            },
            ConsoleGraphicsData::Gba(gba) => match space {
                "vram" => slice_memory_json(space, start, length, &gba.vram),
                "oam" => slice_memory_json(space, start, length, &gba.oam),
                "palette" | "paletteram" => {
                    slice_memory_json(space, start, length, &gba.palette_ram)
                }
                _ => memory_not_ready_json(
                    space,
                    start as u32,
                    length,
                    "unsupported GBA memory space",
                ),
            },
            ConsoleGraphicsData::Nes(nes) => match space {
                "chr" | "chrdata" => slice_memory_json(space, start, length, &nes.chr_data),
                "nametable" | "nametableram" => {
                    slice_memory_json(space, start, length, &nes.nametable_data)
                }
                "oam" | "spriteoam" | "spriteattribute" | "spriteattributememory" => {
                    slice_memory_json(space, start, length, &nes.oam)
                }
                "palette" | "paletteram" => {
                    slice_memory_json(space, start, length, &nes.palette_ram)
                }
                _ => memory_not_ready_json(
                    space,
                    start as u32,
                    length,
                    "unsupported NES memory space",
                ),
            },
            ConsoleGraphicsData::Sega8(sega8) => match space {
                "vram" => slice_memory_json(space, start, length, &sega8.vram),
                "cram" | "palette" | "paletteram" => {
                    slice_memory_json(space, start, length, &sega8.cram)
                }
                "oam" | "sat" | "spriteattribute" | "spriteattributetable" => {
                    slice_memory_json(space, start, length, &sega8.oam)
                }
                _ => memory_not_ready_json(
                    space,
                    start as u32,
                    length,
                    "unsupported Sega 8-bit memory space",
                ),
            },
        }
    }
}

fn normalized_space_name(space: &str) -> String {
    space
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_' | ' ' | '.'))
        .collect()
}

fn memory_not_ready_json(space: &str, start: u32, length: usize, note: &str) -> Value {
    json!({
        "ready": false,
        "space": space,
        "start": start,
        "requested_length": length,
        "note": note,
    })
}

fn memory_ready_json(space: &str, start: u32, available: usize, bytes: &[u8]) -> Value {
    json!({
        "ready": true,
        "space": space,
        "start": start,
        "length": bytes.len(),
        "available": available,
        "hex": hex_string(bytes),
        "bytes": bytes.iter().map(|byte| json!(byte)).collect::<Vec<_>>(),
    })
}

fn slice_memory_json(space: &str, start: usize, length: usize, data: &[u8]) -> Value {
    if start >= data.len() {
        return json!({
            "ready": false,
            "space": space,
            "start": start,
            "requested_length": length,
            "available": data.len(),
            "note": "start is outside the selected buffer",
        });
    }
    let end = start.saturating_add(length).min(data.len());
    memory_ready_json(space, start as u32, data.len(), &data[start..end])
}

fn hex_string(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}
