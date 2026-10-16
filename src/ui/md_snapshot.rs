use crate::debug::{PerfInfo, RomDebugInfo, RomInfoSection};
use crate::emu_thread::SnapshotRequest;
use zeff_emu_common::address::Address;
use zeff_md_core::{Emulator, Region};

mod apu;
mod cpu;
mod disassembly;
mod graphics;

pub(crate) fn collect_md_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    reusable_memory: Option<Vec<(Address, u8)>>,
) -> super::UiFrameData {
    super::UiFrameData {
        apu_debug: snapshot.show_apu_viewer.then(|| apu::snapshot(emu)),
        cpu_debug: snapshot.want_debug_info.then(|| cpu::snapshot(emu)),
        disassembly_view: disassembly::snapshot(emu, snapshot.show_disassembler),
        palette_debug: snapshot.any_viewer_open.then(|| graphics::palette(emu)),
        graphics_data: snapshot
            .any_vram_viewer_open
            .then(|| graphics::snapshot(emu)),
        perf_info: snapshot.want_perf_info.then(|| perf_info(emu)),
        rom_debug: snapshot.show_rom_info.then(|| rom_info(emu)),
        rom_page: super::build_rom_page(
            snapshot.show_rom_viewer,
            snapshot.rom_view_start,
            emu.rom_bytes(),
        ),
        rom_size: emu.rom_bytes().len() as u32,
        rom_search_results: super::build_rom_search(snapshot.rom_search.as_ref(), emu.rom_bytes()),
        memory_page: memory_page(emu, snapshot, reusable_memory),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            &[
                (0, emu.rom_bytes().len() as Address - 1),
                (0xff_0000, 0xff_ffff),
            ],
            |address| emu.peek(address).unwrap_or(0xff),
        ),
        ..Default::default()
    }
}

fn memory_page(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    reusable: Option<Vec<(Address, u8)>>,
) -> Option<Vec<(Address, u8)>> {
    if !snapshot.show_memory_viewer {
        return reusable.map(|mut page| {
            page.clear();
            page
        });
    }
    let mut page = reusable.unwrap_or_else(|| Vec::with_capacity(256));
    page.clear();
    let start = snapshot.memory_view_start;
    if start <= 0xff_ffff {
        for address in start..=start.saturating_add(255).min(0xff_ffff) {
            // Unmapped bytes are a display placeholder, not an emulated bus read.
            page.push((address, emu.peek(address).unwrap_or(0xff)));
        }
    }
    Some(page)
}

fn region_label(region: Region) -> &'static str {
    match region {
        Region::NtscUs => "NTSC (US)",
        Region::NtscJapan => "NTSC (Japan)",
        Region::Pal => "PAL (Europe)",
    }
}

fn perf_info(emu: &Emulator) -> PerfInfo {
    PerfInfo {
        fps: 0.0,
        target_fps: emu.region().master_hz() as f64 / emu.region().frame_ticks() as f64,
        speed_mode_label: super::normal_speed_mode_label(),
        frames_in_flight: 0,
        cycles: emu.cpu_clock().0,
        platform_name: "Mega Drive",
        hardware_label: "Mega Drive / Genesis".into(),
        hardware_pref_label: region_label(emu.region()).into(),
    }
}

fn rom_info(emu: &Emulator) -> RomDebugInfo {
    let mut sections = vec![RomInfoSection {
        heading: "Mega Drive Cartridge",
        fields: vec![
            ("Selected console region", region_label(emu.region()).into()),
            ("ROM size", format!("{} bytes", emu.rom_bytes().len())),
        ],
    }];
    if let Some(header) = emu.cartridge_header() {
        let range = |(start, end)| format!("{start:06X}..{end:06X}");
        let save = if emu.save_ram_kind().is_persistent() {
            format!("{} bytes (persistent)", emu.save_ram_kind().size())
        } else if header.declares_save_hardware() {
            "Declared (unsupported)".into()
        } else {
            "Not declared".into()
        };
        sections.push(RomInfoSection {
            heading: "Cartridge Header",
            fields: vec![
                ("Console", header.console.clone()),
                ("Copyright", header.copyright.clone()),
                ("Domestic title", header.domestic_title.clone()),
                ("International title", header.international_title.clone()),
                ("Product / revision", header.product.clone()),
                ("Declared countries", header.countries.clone()),
                ("Peripherals", header.peripherals.clone()),
                ("Declared ROM range", range(header.rom_range)),
                ("Declared work RAM range", range(header.work_ram_range)),
                ("Header checksum", format!("{:04X}", header.checksum)),
                (
                    "Calculated checksum",
                    format!("{:04X}", header.calculated_checksum),
                ),
                ("Cartridge save hardware", save),
            ],
        });
    } else {
        sections.push(RomInfoSection {
            heading: "Cartridge Header",
            fields: vec![("Status", "ROM is shorter than the header".into())],
        });
    }
    RomDebugInfo { sections }
}

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_md_core::LoadConfig;

    #[test]
    fn metadata_keeps_declared_countries_separate_from_selected_console() {
        let mut bytes = vec![0; 0x204];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        bytes[0x120..0x125].copy_from_slice(b"Local");
        bytes[0x150..0x156].copy_from_slice(b"Export");
        bytes[0x1f0] = b'J';
        bytes[0x200..].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        let emu = Emulator::new(bytes, LoadConfig::default()).unwrap();
        let info = rom_info(&emu);
        let field = |name| {
            info.sections
                .iter()
                .flat_map(|section| &section.fields)
                .find(|(key, _)| *key == name)
                .unwrap()
                .1
                .as_str()
        };
        assert_eq!(field("Selected console region"), "NTSC (US)");
        assert_eq!(field("Declared countries"), "J");
        assert_eq!(field("Domestic title"), "Local");
        assert_eq!(field("International title"), "Export");
        assert_eq!(field("ROM size"), "516 bytes");
        let perf = perf_info(&emu);
        assert_eq!(perf.hardware_pref_label, "NTSC (US)");
        assert_eq!(
            perf.target_fps,
            Region::NtscUs.master_hz() as f64 / Region::NtscUs.frame_ticks() as f64
        );
        let mut bytes = emu.rom_bytes().to_vec();
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
        let persistent = Emulator::new(bytes, LoadConfig::default()).unwrap();
        assert!(
            rom_info(&persistent)
                .sections
                .iter()
                .flat_map(|section| &section.fields)
                .any(|(key, value)| *key == "Cartridge save hardware"
                    && value == "512 bytes (persistent)")
        );
    }
}
