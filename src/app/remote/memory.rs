use serde_json::{Value, json};
use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};

use crate::app::App;
use crate::debug::ConsoleGraphicsData;
use crate::live_control::LiveMemorySpace;

impl App {
    pub(super) fn live_memory_json(
        &mut self,
        requested_space: &LiveMemorySpace,
        start: u32,
        length: usize,
    ) -> Value {
        let raw_space = requested_space.request_name();
        let normalized = normalized_space_name(raw_space);
        let regions = self
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.core_features.as_ref())
            .map(|features| features.memory_regions.as_slice())
            .unwrap_or(&[]);
        let space = match requested_space {
            LiveMemorySpace::Cpu => "cpu".to_string(),
            LiveMemorySpace::Region(_) => {
                canonical_cached_memory_space(raw_space, &normalized, regions)
            }
        };

        if space == "cpu" {
            self.remote_memory_view_start = Some(start);
            self.remote_memory_frames_remaining = 3;
            return self.live_cpu_memory_json(start, length.min(256));
        }

        if matches!(space.as_str(), "systemram" | "saveram" | "ioregisters") {
            return memory_not_ready_json(
                &space,
                start,
                length,
                "advertised memory region is not cached by live memory yet",
            );
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
                "vram" | "chr" | "chrdata" => {
                    slice_memory_json(space, start, length, &nes.chr_data)
                }
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

fn canonical_cached_memory_space(
    raw_space: &str,
    normalized_space: &str,
    regions: &[MemoryRegionDescriptor],
) -> String {
    if is_legacy_cpu_space(normalized_space) {
        return "cpu".to_string();
    }

    if let Some(region) = resolve_memory_region(regions, raw_space)
        .or_else(|| resolve_memory_region(regions, normalized_space))
    {
        return match region.kind {
            MemoryRegionKind::CpuAddressSpace => "cpu",
            MemoryRegionKind::VideoRam => "vram",
            MemoryRegionKind::PaletteRam => "palette",
            MemoryRegionKind::Oam => "oam",
            MemoryRegionKind::Framebuffer => "framebuffer",
            MemoryRegionKind::SystemRam => "systemram",
            MemoryRegionKind::ExternalWorkRam => "ewram",
            MemoryRegionKind::InternalWorkRam => "iwram",
            MemoryRegionKind::SaveRam => "saveram",
            MemoryRegionKind::IoRegisters => "ioregisters",
        }
        .to_string();
    }

    normalized_space.to_string()
}

fn is_legacy_cpu_space(space: &str) -> bool {
    matches!(space, "cpu" | "memory" | "ram")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_cached_memory_space_uses_region_aliases() {
        let regions = [
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::video_ram(0x2000),
            MemoryRegionDescriptor::palette_ram(32),
            MemoryRegionDescriptor::oam(256),
            MemoryRegionDescriptor::io_registers(0x400),
            MemoryRegionDescriptor::save_ram(0x2000),
            MemoryRegionDescriptor::framebuffer(160 * 144 * 4),
        ];

        assert_eq!(
            canonical_cached_memory_space(
                "video ram",
                &normalized_space_name("video ram"),
                &regions
            ),
            "vram"
        );
        assert_eq!(
            canonical_cached_memory_space("CRAM", &normalized_space_name("CRAM"), &regions),
            "palette"
        );
        assert_eq!(
            canonical_cached_memory_space(
                "sprite_ram",
                &normalized_space_name("sprite_ram"),
                &regions
            ),
            "oam"
        );
        assert_eq!(
            canonical_cached_memory_space("frame", &normalized_space_name("frame"), &regions),
            "framebuffer"
        );
        assert_eq!(
            canonical_cached_memory_space(
                "io registers",
                &normalized_space_name("io registers"),
                &regions
            ),
            "ioregisters"
        );
        assert_eq!(
            canonical_cached_memory_space("save_ram", &normalized_space_name("save_ram"), &regions),
            "saveram"
        );
    }

    #[test]
    fn canonical_cached_memory_space_preserves_legacy_ram_as_cpu() {
        let regions = [
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(0x2000),
        ];

        assert_eq!(
            canonical_cached_memory_space("ram", &normalized_space_name("ram"), &regions),
            "cpu"
        );
        assert_eq!(
            canonical_cached_memory_space(
                "system_ram",
                &normalized_space_name("system_ram"),
                &regions
            ),
            "systemram"
        );
    }
}
