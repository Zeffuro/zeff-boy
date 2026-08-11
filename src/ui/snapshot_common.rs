use crate::debug::{
    DisassemblyView, MemorySearchResult, RomInfoSection, RomSearchResult, WatchHitDisplay,
    WatchpointDisplay,
};
use crate::emu_thread::MemorySearchRequest;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::WatchType;

pub(crate) fn build_memory_page(
    show: bool,
    start: Address,
    reusable: Option<Vec<(Address, u8)>>,
    peek: impl Fn(Address) -> u8,
) -> Option<Vec<(Address, u8)>> {
    if show {
        let mut page = reusable.unwrap_or_else(|| Vec::with_capacity(256));
        page.clear();
        for i in 0..256u32 {
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

pub(crate) fn build_rom_page(show: bool, start: u32, rom_bytes: &[u8]) -> Option<Vec<(u32, u8)>> {
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

pub(crate) fn build_disassembly_view(
    show: bool,
    last_pc: Option<Address>,
    current_pc: Address,
    disassemble: impl FnOnce() -> Vec<crate::debug::DisassembledLine>,
    breakpoints_iter: impl Iterator<Item = Address>,
) -> Option<DisassemblyView> {
    if !show {
        return None;
    }
    if last_pc == Some(current_pc) {
        return None;
    }
    let mut breakpoints: Vec<Address> = breakpoints_iter.collect();
    breakpoints.sort_unstable();
    Some(DisassemblyView {
        pc: current_pc,
        lines: disassemble(),
        breakpoints,
    })
}

pub(crate) struct DebugControlSnapshot {
    pub(crate) breakpoints: Vec<Address>,
    pub(crate) watchpoints: Vec<WatchpointDisplay>,
    pub(crate) hit_breakpoint: Option<Address>,
    pub(crate) hit_watchpoint: Option<WatchHitDisplay>,
}

pub(crate) fn build_debug_control_snapshot(
    breakpoints: impl IntoIterator<Item = Address>,
    watchpoints: impl IntoIterator<Item = (Address, WatchType)>,
    hit_breakpoint: Option<Address>,
    hit_watchpoint: Option<(Address, u8, u8, WatchType)>,
) -> DebugControlSnapshot {
    let mut breakpoints: Vec<Address> = breakpoints.into_iter().collect();
    breakpoints.sort_unstable();

    DebugControlSnapshot {
        breakpoints,
        watchpoints: watchpoints
            .into_iter()
            .map(|(address, watch_type)| WatchpointDisplay {
                address,
                watch_type,
            })
            .collect(),
        hit_breakpoint,
        hit_watchpoint: hit_watchpoint.map(|(address, old_value, new_value, watch_type)| {
            WatchHitDisplay {
                address,
                old_value,
                new_value,
                watch_type,
            }
        }),
    }
}

pub(crate) fn normal_speed_mode_label() -> &'static str {
    "1Ã—"
}

pub(crate) fn build_memory_search(
    search: Option<&MemorySearchRequest>,
    peek: impl Fn(Address) -> u8,
) -> Option<Vec<MemorySearchResult>> {
    build_memory_search_ranges(search, &[(0, 0xFFFF)], peek)
}

pub(crate) fn build_memory_search_ranges(
    search: Option<&MemorySearchRequest>,
    ranges: &[(Address, Address)],
    peek: impl Fn(Address) -> u8,
) -> Option<Vec<MemorySearchResult>> {
    let search = search?;
    let mut results = Vec::new();
    if !search.pattern.is_empty() {
        let pattern_len = search.pattern.len() as Address;
        for &(range_start, range_end) in ranges {
            let Some(last_start) = pattern_len
                .checked_sub(1)
                .and_then(|tail| range_end.checked_sub(tail))
            else {
                continue;
            };
            if pattern_len == 0 || last_start < range_start {
                continue;
            }
            let mut start_addr = range_start;
            while start_addr <= last_start {
                if results.len() >= search.max_results {
                    break;
                }
                let matched =
                    search.pattern.iter().enumerate().all(|(j, expected)| {
                        peek(start_addr.wrapping_add(j as Address)) == *expected
                    });
                if matched {
                    results.push(MemorySearchResult {
                        address: start_addr,
                        matched_bytes: search.pattern.clone(),
                    });
                }
                let Some(next) = start_addr.checked_add(1) else {
                    break;
                };
                start_addr = next;
            }
            if results.len() >= search.max_results {
                break;
            }
        }
    }
    Some(results)
}

pub(crate) fn build_rom_search(
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

pub(crate) fn build_libretro_section(
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
