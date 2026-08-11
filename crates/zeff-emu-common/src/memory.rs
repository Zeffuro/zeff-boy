#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub enum MemoryRegionKind {
    CpuAddressSpace,
    SystemRam,
    ExternalWorkRam,
    InternalWorkRam,
    VideoRam,
    PaletteRam,
    Oam,
    IoRegisters,
    SaveRam,
    Framebuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub enum MemoryRegionView {
    AddressSpace,
    Physical,
    Aggregate,
    Derived,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtendedMemoryRegionSizes {
    pub external_work_ram_len: usize,
    pub internal_work_ram_len: usize,
    pub palette_ram_len: usize,
    pub oam_len: usize,
    pub io_registers_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRegionDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: MemoryRegionKind,
    pub size: Option<usize>,
    pub address_bits: Option<u8>,
    pub readable: bool,
    pub writable: bool,
    pub side_effect_free: bool,
    pub copyable: bool,
    pub view: MemoryRegionView,
    pub aliases: &'static [&'static str],
}

const CPU_ALIASES: &[&str] = &["memory", "ram"];
const SYSTEM_RAM_ALIASES: &[&str] = &["systemram", "wram"];
const EXTERNAL_WORK_RAM_ALIASES: &[&str] = &["ewram", "externalwram", "external_work_ram"];
const INTERNAL_WORK_RAM_ALIASES: &[&str] = &["iwram", "internalwram", "internal_work_ram"];
const VIDEO_RAM_ALIASES: &[&str] = &["vram", "chr", "chrdata"];
const PALETTE_RAM_ALIASES: &[&str] = &["palette", "paletteram", "palette_ram", "cram"];
const OAM_ALIASES: &[&str] = &["sprites", "sprite_ram", "obj", "objects"];
const IO_REGISTER_ALIASES: &[&str] = &["io", "ioreg", "io_registers", "registers"];
const SAVE_RAM_ALIASES: &[&str] = &["saveram", "sram", "battery"];
const FRAMEBUFFER_ALIASES: &[&str] = &["frame", "fb"];

impl MemoryRegionDescriptor {
    pub const fn cpu_address_space(address_bits: u8) -> Self {
        Self {
            id: "cpu",
            label: "CPU address space",
            kind: MemoryRegionKind::CpuAddressSpace,
            size: None,
            address_bits: Some(address_bits),
            readable: true,
            writable: true,
            side_effect_free: true,
            copyable: false,
            view: MemoryRegionView::AddressSpace,
            aliases: CPU_ALIASES,
        }
    }

    pub const fn system_ram(size: usize) -> Self {
        Self {
            id: "system_ram",
            label: "System RAM",
            kind: MemoryRegionKind::SystemRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: SYSTEM_RAM_ALIASES,
        }
    }

    pub const fn aggregate_system_ram(size: usize) -> Self {
        Self {
            id: "system_ram",
            label: "System RAM",
            kind: MemoryRegionKind::SystemRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Aggregate,
            aliases: SYSTEM_RAM_ALIASES,
        }
    }

    pub const fn external_work_ram(size: usize) -> Self {
        Self {
            id: "ewram",
            label: "External Work RAM",
            kind: MemoryRegionKind::ExternalWorkRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: EXTERNAL_WORK_RAM_ALIASES,
        }
    }

    pub const fn internal_work_ram(size: usize) -> Self {
        Self {
            id: "iwram",
            label: "Internal Work RAM",
            kind: MemoryRegionKind::InternalWorkRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: INTERNAL_WORK_RAM_ALIASES,
        }
    }

    pub const fn video_ram(size: usize) -> Self {
        Self {
            id: "video_ram",
            label: "Video RAM",
            kind: MemoryRegionKind::VideoRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: VIDEO_RAM_ALIASES,
        }
    }

    pub const fn palette_ram(size: usize) -> Self {
        Self {
            id: "palette_ram",
            label: "Palette RAM",
            kind: MemoryRegionKind::PaletteRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: PALETTE_RAM_ALIASES,
        }
    }

    pub const fn oam(size: usize) -> Self {
        Self {
            id: "oam",
            label: "Object Attribute Memory",
            kind: MemoryRegionKind::Oam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: OAM_ALIASES,
        }
    }

    pub const fn io_registers(size: usize) -> Self {
        Self {
            id: "io_registers",
            label: "I/O registers",
            kind: MemoryRegionKind::IoRegisters,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: IO_REGISTER_ALIASES,
        }
    }

    pub const fn save_ram(size: usize) -> Self {
        Self {
            id: "save_ram",
            label: "Save RAM",
            kind: MemoryRegionKind::SaveRam,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Physical,
            aliases: SAVE_RAM_ALIASES,
        }
    }

    pub const fn framebuffer(size: usize) -> Self {
        Self {
            id: "framebuffer",
            label: "Framebuffer",
            kind: MemoryRegionKind::Framebuffer,
            size: Some(size),
            address_bits: None,
            readable: true,
            writable: false,
            side_effect_free: true,
            copyable: true,
            view: MemoryRegionView::Derived,
            aliases: FRAMEBUFFER_ALIASES,
        }
    }

    pub fn matches_id_or_alias(self, value: &str) -> bool {
        let normalized = normalize_memory_region_name(value);
        normalized == normalize_memory_region_name(self.id)
            || self
                .aliases
                .iter()
                .any(|alias| normalized == normalize_memory_region_name(alias))
    }
}

pub fn normalize_memory_region_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_' | ' ' | '.'))
        .collect()
}

pub fn resolve_memory_region(
    regions: &[MemoryRegionDescriptor],
    id_or_alias: &str,
) -> Option<MemoryRegionDescriptor> {
    regions
        .iter()
        .copied()
        .find(|region| region.matches_id_or_alias(id_or_alias))
}

pub fn standard_memory_regions(
    cpu_address_bits: u8,
    system_ram_len: usize,
    video_ram_len: usize,
    save_ram_kind: crate::save_ram::SaveRamKind,
    framebuffer_len: usize,
) -> Vec<MemoryRegionDescriptor> {
    standard_memory_regions_with_extended(
        cpu_address_bits,
        system_ram_len,
        video_ram_len,
        save_ram_kind,
        framebuffer_len,
        ExtendedMemoryRegionSizes::default(),
    )
}

pub fn standard_memory_regions_with_extended(
    cpu_address_bits: u8,
    system_ram_len: usize,
    video_ram_len: usize,
    save_ram_kind: crate::save_ram::SaveRamKind,
    framebuffer_len: usize,
    extended: ExtendedMemoryRegionSizes,
) -> Vec<MemoryRegionDescriptor> {
    let mut regions = vec![MemoryRegionDescriptor::cpu_address_space(cpu_address_bits)];

    if system_ram_len > 0 {
        let has_physical_work_ram =
            extended.external_work_ram_len > 0 || extended.internal_work_ram_len > 0;
        let system_ram = if has_physical_work_ram {
            MemoryRegionDescriptor::aggregate_system_ram(system_ram_len)
        } else {
            MemoryRegionDescriptor::system_ram(system_ram_len)
        };
        regions.push(system_ram);
    }

    if extended.external_work_ram_len > 0 {
        regions.push(MemoryRegionDescriptor::external_work_ram(
            extended.external_work_ram_len,
        ));
    }

    if extended.internal_work_ram_len > 0 {
        regions.push(MemoryRegionDescriptor::internal_work_ram(
            extended.internal_work_ram_len,
        ));
    }

    if video_ram_len > 0 {
        regions.push(MemoryRegionDescriptor::video_ram(video_ram_len));
    }

    if extended.palette_ram_len > 0 {
        regions.push(MemoryRegionDescriptor::palette_ram(
            extended.palette_ram_len,
        ));
    }

    if extended.oam_len > 0 {
        regions.push(MemoryRegionDescriptor::oam(extended.oam_len));
    }

    if extended.io_registers_len > 0 {
        regions.push(MemoryRegionDescriptor::io_registers(
            extended.io_registers_len,
        ));
    }

    if save_ram_kind.has_ram() {
        regions.push(MemoryRegionDescriptor::save_ram(save_ram_kind.size()));
    }

    regions.push(MemoryRegionDescriptor::framebuffer(framebuffer_len));
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_region_uses_address_bits_not_byte_size() {
        let region = MemoryRegionDescriptor::cpu_address_space(32);

        assert_eq!(region.id, "cpu");
        assert_eq!(region.size, None);
        assert_eq!(region.address_bits, Some(32));
        assert!(region.readable);
        assert!(region.writable);
        assert!(region.side_effect_free);
        assert!(!region.copyable);
        assert_eq!(region.view, MemoryRegionView::AddressSpace);
        assert!(region.matches_id_or_alias("memory"));
        assert!(region.matches_id_or_alias("CPU"));
    }

    #[test]
    fn fixed_regions_report_lengths() {
        let region = MemoryRegionDescriptor::video_ram(0x2000);

        assert_eq!(region.kind, MemoryRegionKind::VideoRam);
        assert_eq!(region.size, Some(0x2000));
        assert!(region.readable);
        assert!(!region.writable);
        assert!(region.side_effect_free);
        assert!(region.copyable);
        assert_eq!(region.view, MemoryRegionView::Physical);
        assert!(region.matches_id_or_alias("vram"));
        assert!(region.matches_id_or_alias("chr"));
        assert!(region.matches_id_or_alias("video-ram"));
    }

    #[test]
    fn extended_regions_report_lengths_and_aliases() {
        let palette = MemoryRegionDescriptor::palette_ram(0x400);
        let oam = MemoryRegionDescriptor::oam(0x100);
        let io = MemoryRegionDescriptor::io_registers(0x40);

        assert_eq!(palette.kind, MemoryRegionKind::PaletteRam);
        assert_eq!(palette.size, Some(0x400));
        assert!(palette.matches_id_or_alias("cram"));

        assert_eq!(oam.kind, MemoryRegionKind::Oam);
        assert_eq!(oam.size, Some(0x100));
        assert!(oam.matches_id_or_alias("sprites"));

        assert_eq!(io.kind, MemoryRegionKind::IoRegisters);
        assert_eq!(io.size, Some(0x40));
        assert_eq!(io.view, MemoryRegionView::Physical);
        assert!(io.matches_id_or_alias("io"));
    }

    #[test]
    fn resolves_region_by_normalized_id_or_alias() {
        let regions = standard_memory_regions_with_extended(
            16,
            0x2000,
            0x4000,
            crate::save_ram::SaveRamKind::none(),
            160 * 144 * 4,
            ExtendedMemoryRegionSizes {
                external_work_ram_len: 0,
                internal_work_ram_len: 0,
                palette_ram_len: 0x40,
                oam_len: 0x100,
                io_registers_len: 0,
            },
        );

        assert_eq!(
            resolve_memory_region(&regions, "video ram").map(|region| region.id),
            Some("video_ram")
        );
        assert_eq!(
            resolve_memory_region(&regions, "CRAM").map(|region| region.id),
            Some("palette_ram")
        );
        assert_eq!(resolve_memory_region(&regions, "missing"), None);
    }

    #[test]
    fn standard_regions_include_optional_save_ram() {
        let regions = standard_memory_regions(
            16,
            0x2000,
            0x4000,
            crate::save_ram::SaveRamKind::mapper_ram_unknown(0x8000),
            160 * 144 * 4,
        );

        assert!(regions.iter().any(|region| region.id == "cpu"));
        assert!(regions.iter().any(|region| region.id == "system_ram"));
        assert!(regions.iter().any(|region| region.id == "video_ram"));
        assert!(regions.iter().any(|region| region.id == "save_ram"));
        assert!(regions.iter().any(|region| region.id == "framebuffer"));
    }

    #[test]
    fn standard_regions_include_extended_regions_when_requested() {
        let regions = standard_memory_regions_with_extended(
            32,
            0x48000,
            0x18000,
            crate::save_ram::SaveRamKind::none(),
            240 * 160 * 4,
            ExtendedMemoryRegionSizes {
                external_work_ram_len: 0x40000,
                internal_work_ram_len: 0x8000,
                palette_ram_len: 0x400,
                oam_len: 0x400,
                io_registers_len: 0x400,
            },
        );

        assert!(regions.iter().any(|region| region.id == "ewram"));
        assert!(regions.iter().any(|region| region.id == "iwram"));
        assert!(regions.iter().any(|region| region.id == "palette_ram"));
        assert!(regions.iter().any(|region| region.id == "oam"));
        assert!(regions.iter().any(|region| region.id == "io_registers"));
        assert!(!regions.iter().any(|region| region.id == "save_ram"));
        assert_eq!(
            resolve_memory_region(&regions, "system_ram").map(|region| region.view),
            Some(MemoryRegionView::Aggregate)
        );
        assert_eq!(
            resolve_memory_region(&regions, "ewram").map(|region| region.view),
            Some(MemoryRegionView::Physical)
        );
        assert_eq!(
            resolve_memory_region(&regions, "framebuffer").map(|region| region.view),
            Some(MemoryRegionView::Derived)
        );
    }
}
