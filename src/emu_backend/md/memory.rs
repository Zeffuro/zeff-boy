use zeff_emu_common::memory::{MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region};
use zeff_md_core::Emulator;

use crate::emu_core_trait::{copy_optional_region_to_vec, copy_slice_to_vec};

pub(super) fn regions(emu: &Emulator) -> Vec<MemoryRegionDescriptor> {
    let memory = emu.vdp().memory();
    let mut regions = vec![
        MemoryRegionDescriptor::read_only_cpu_address_space(24),
        MemoryRegionDescriptor::system_ram(emu.work_ram().len()),
        MemoryRegionDescriptor::video_ram(memory.vram().len()),
        MemoryRegionDescriptor::palette_ram(memory.cram_word_count() * size_of::<u16>()),
        MemoryRegionDescriptor::video_scroll_ram(size_of_val(memory.vsram())),
    ];
    if emu.save_ram_kind().has_ram() {
        regions.push(MemoryRegionDescriptor::save_ram(emu.save_ram_kind().size()));
    }
    regions.push(MemoryRegionDescriptor::framebuffer(emu.framebuffer().len()));
    regions
}

pub(super) fn copy(
    emu: &Emulator,
    id_or_alias: &str,
    out: &mut Vec<u8>,
) -> anyhow::Result<MemoryRegionDescriptor> {
    let region = resolve_memory_region(&regions(emu), id_or_alias)
        .ok_or_else(|| anyhow::anyhow!("unknown Mega Drive memory region '{id_or_alias}'"))?;
    match region.kind {
        MemoryRegionKind::SystemRam => copy_slice_to_vec(out, emu.work_ram()),
        MemoryRegionKind::VideoRam => copy_slice_to_vec(out, emu.vdp().memory().vram()),
        MemoryRegionKind::PaletteRam => copy_words(
            out,
            emu.vdp()
                .memory()
                .known_cram()
                .ok_or_else(|| anyhow::anyhow!("Mega Drive CRAM contains unknown words"))?,
        ),
        MemoryRegionKind::VideoScrollRam => copy_words(out, emu.vdp().memory().vsram()),
        MemoryRegionKind::SaveRam => {
            copy_optional_region_to_vec(out, emu.dump_save_ram(), region.id)?;
        }
        MemoryRegionKind::Framebuffer => {
            anyhow::ensure!(
                emu.framebuffer_available(),
                "Mega Drive framebuffer is unavailable"
            );
            copy_slice_to_vec(out, emu.framebuffer());
        }
        MemoryRegionKind::CpuAddressSpace => {
            anyhow::bail!("Mega Drive CPU address space is not copyable as a finite memory region");
        }
        _ => anyhow::bail!("Mega Drive memory region '{}' is not copyable", region.id),
    }
    Ok(region)
}

fn copy_words(out: &mut Vec<u8>, words: &[u16]) {
    out.clear();
    out.extend(words.iter().flat_map(|word| word.to_be_bytes()));
}
