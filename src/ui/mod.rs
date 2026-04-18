use crate::debug::{
    ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugUiActions, DisassemblyView,
    InputDebugInfo, MemorySearchResult, OamDebugInfo, PaletteDebugInfo, PerfInfo, RomDebugInfo,
    RomInfoSection, RomSearchResult,
};
use crate::emu_thread::MemorySearchRequest;

mod gb_snapshot;
mod nes_snapshot;

pub(crate) use gb_snapshot::collect_emu_snapshot;
pub(crate) use nes_snapshot::collect_nes_snapshot;

fn build_memory_page(
    show: bool,
    start: u16,
    reusable: Option<Vec<(u16, u8)>>,
    peek: impl Fn(u16) -> u8,
) -> Option<Vec<(u16, u8)>> {
    if show {
        let mut page = reusable.unwrap_or_else(|| Vec::with_capacity(256));
        page.clear();
        for i in 0..256u16 {
            let addr = start.wrapping_add(i);
            page.push((addr, peek(addr)));
        }
        Some(page)
    } else {
        reusable.map(|mut v| {
            v.clear();
            v
        })
    }
}

fn build_rom_page(show: bool, start: u32, rom_bytes: &[u8]) -> Option<Vec<(u32, u8)>> {
    if !show {
        return None;
    }
    let start = start as usize;
    let mut buf = Vec::with_capacity(256);
    for i in 0..256usize {
        let offset = start + i;
        if offset < rom_bytes.len() {
            buf.push((offset as u32, rom_bytes[offset]));
        }
    }
    Some(buf)
}

fn build_disassembly_view(
    show: bool,
    last_pc: Option<u16>,
    current_pc: u16,
    disassemble: impl FnOnce() -> Vec<crate::debug::DisassembledLine>,
    breakpoints_iter: impl Iterator<Item = u16>,
) -> Option<DisassemblyView> {
    if !show {
        return None;
    }
    if last_pc == Some(current_pc) {
        return None;
    }
    let mut breakpoints: Vec<u16> = breakpoints_iter.collect();
    breakpoints.sort_unstable();
    Some(DisassemblyView {
        pc: current_pc,
        lines: disassemble(),
        breakpoints,
    })
}

fn build_memory_search(
    search: Option<&MemorySearchRequest>,
    peek: impl Fn(u16) -> u8,
) -> Option<Vec<MemorySearchResult>> {
    let search = search?;
    let mut results = Vec::new();
    if !search.pattern.is_empty() {
        let pattern_len = search.pattern.len();
        for start_addr in 0..=(0x10000usize - pattern_len) {
            if results.len() >= search.max_results {
                break;
            }
            let matched =
                (0..pattern_len).all(|j| peek((start_addr + j) as u16) == search.pattern[j]);
            if matched {
                results.push(MemorySearchResult {
                    address: start_addr as u16,
                    matched_bytes: search.pattern.clone(),
                });
            }
        }
    }
    Some(results)
}

fn build_rom_search(
    search: Option<&MemorySearchRequest>,
    rom_bytes: &[u8],
) -> Option<Vec<RomSearchResult>> {
    let search = search?;
    let mut results = Vec::new();
    if !search.pattern.is_empty() {
        let pattern_len = search.pattern.len();
        let end = rom_bytes
            .len()
            .saturating_sub(pattern_len.saturating_sub(1));
        for start_offset in 0..end {
            if results.len() >= search.max_results {
                break;
            }
            if rom_bytes[start_offset..start_offset + pattern_len] == search.pattern[..] {
                results.push(RomSearchResult {
                    offset: start_offset as u32,
                    matched_bytes: search.pattern.clone(),
                });
            }
        }
    }
    Some(results)
}

fn build_libretro_section(
    rom_crc32: u32,
    platform: crate::libretro_common::LibretroPlatform,
) -> RomInfoSection {
    let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, platform);
    let fields = match &libretro_meta {
        Some(meta) => vec![
            ("Title", meta.title.clone()),
            ("ROM File", meta.rom_name.clone()),
        ],
        None => vec![("Status", "No local metadata match".into())],
    };
    RomInfoSection {
        heading: "libretro Metadata",
        fields,
    }
}

#[derive(Default)]
pub(crate) struct UiFrameData {
    pub(crate) cpu_debug: Option<CpuDebugSnapshot>,
    pub(crate) perf_info: Option<PerfInfo>,
    pub(crate) apu_debug: Option<ApuDebugInfo>,
    pub(crate) oam_debug: Option<OamDebugInfo>,
    pub(crate) palette_debug: Option<PaletteDebugInfo>,
    pub(crate) rom_debug: Option<RomDebugInfo>,
    pub(crate) input_debug: Option<InputDebugInfo>,
    pub(crate) graphics_data: Option<ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
    pub(crate) memory_search_results: Option<Vec<MemorySearchResult>>,
    pub(crate) rom_page: Option<Vec<(u32, u8)>>,
    pub(crate) rom_size: u32,
    pub(crate) rom_search_results: Option<Vec<RomSearchResult>>,
}

pub(crate) fn apply_debug_actions(
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
    backstep_requested: &mut bool,
) {
    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
    if actions.backstep_requested {
        *backstep_requested = true;
    }
}
