use super::{ActiveCore, CoreState};
use zeff_emu_common::memory::{
    ExtendedMemoryRegionSizes, MemoryRegionDescriptor, MemoryRegionKind, resolve_memory_region,
    standard_memory_regions_with_extended,
};
use zeff_emu_common::save_ram::SaveRamKind;

impl CoreState {
    pub fn battery_sram(&self) -> Option<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.dump_battery_sram(),
            ActiveCore::Gba(emu) => emu.dump_battery_sram(),
            ActiveCore::Nes(emu) => emu.dump_battery_sram(),
            ActiveCore::Pce(_) => None,
            ActiveCore::Sega8(emu) => emu.dump_battery_sram(),
            ActiveCore::Ws(emu) => emu.dump_battery_sram(),
        }
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        match &self.core {
            ActiveCore::Gb(emu) => emu.save_ram_kind(),
            ActiveCore::Gba(emu) => emu.save_ram_kind(),
            ActiveCore::Nes(emu) => emu.save_ram_kind(),
            ActiveCore::Pce(host) => host.save_ram_kind(),
            ActiveCore::Sega8(emu) => emu.save_ram_kind(),
            ActiveCore::Ws(emu) => emu.save_ram_kind(),
        }
    }

    #[allow(dead_code)]
    pub fn load_battery_sram(&mut self, data: &[u8]) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Gba(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Nes(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Pce(_) => {}
            ActiveCore::Sega8(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Ws(emu) => {
                let _ = emu.load_battery_sram(data);
            }
        }
    }

    pub fn sync_sram_to_buf(&self, buf: &mut Vec<u8>) {
        if let Some(sram) = self.battery_sram() {
            buf.resize(sram.len(), 0);
            buf.copy_from_slice(&sram);
        }
    }

    #[allow(dead_code)]
    pub fn load_sram_from_buf(&mut self, buf: &[u8]) {
        if !buf.is_empty() {
            self.load_battery_sram(buf);
        }
    }

    pub fn sram_size(&self) -> usize {
        let kind = self.save_ram_kind();
        if kind.is_battery_backed() {
            kind.size()
        } else {
            0
        }
    }

    pub fn memory_regions(&self) -> Vec<MemoryRegionDescriptor> {
        standard_memory_regions_with_extended(
            self.cpu_address_bits(),
            self.system_ram_size(),
            self.video_ram_size(),
            self.save_ram_kind(),
            self.framebuffer_len(),
            ExtendedMemoryRegionSizes {
                external_work_ram_len: self.external_work_ram_size(),
                internal_work_ram_len: self.internal_work_ram_size(),
                palette_ram_len: self.palette_ram_size(),
                oam_len: self.oam_size(),
                io_registers_len: self.io_registers_size(),
            },
        )
    }

    pub fn memory_region_size(&self, kind: MemoryRegionKind) -> usize {
        self.memory_regions()
            .iter()
            .find(|region| region.kind == kind)
            .and_then(|region| region.size)
            .unwrap_or(0)
    }

    #[allow(dead_code)]
    pub fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<MemoryRegionDescriptor> {
        let regions = self.memory_regions();
        let region = resolve_memory_region(&regions, id_or_alias)
            .ok_or_else(|| anyhow::anyhow!("unknown libretro memory region '{id_or_alias}'"))?;

        match (&mut self.core, region.kind) {
            (_, MemoryRegionKind::CpuAddressSpace) => Err(anyhow::anyhow!(
                "CPU address space is debugger-addressable, not copyable as a finite memory region"
            )),
            (ActiveCore::Gb(emu), MemoryRegionKind::SystemRam) => {
                copy_slice_to_vec(out, emu.system_ram());
                Ok(region)
            }
            (ActiveCore::Gb(emu), MemoryRegionKind::VideoRam) => {
                copy_slice_to_vec(out, emu.video_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Gb(emu), MemoryRegionKind::SaveRam) => {
                copy_optional_region(out, emu.dump_battery_sram(), region.id)?;
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::SystemRam) => {
                let (ewram, iwram) = emu.system_ram();
                out.clear();
                out.extend_from_slice(ewram);
                out.extend_from_slice(iwram);
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::ExternalWorkRam) => {
                copy_slice_to_vec(out, emu.system_ram().0);
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::InternalWorkRam) => {
                copy_slice_to_vec(out, emu.system_ram().1);
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::VideoRam) => {
                copy_slice_to_vec(out, emu.video_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::PaletteRam) => {
                copy_slice_to_vec(out, emu.palette_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::Oam) => {
                copy_slice_to_vec(out, emu.oam_snapshot());
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::IoRegisters) => {
                copy_slice_to_vec(out, emu.io_snapshot());
                Ok(region)
            }
            (ActiveCore::Gba(emu), MemoryRegionKind::SaveRam) => {
                copy_optional_region(out, emu.dump_battery_sram(), region.id)?;
                Ok(region)
            }
            (ActiveCore::Nes(emu), MemoryRegionKind::SystemRam) => {
                copy_slice_to_vec(out, emu.system_ram());
                Ok(region)
            }
            (ActiveCore::Nes(emu), MemoryRegionKind::VideoRam) => {
                let video_ram = emu.video_ram_snapshot();
                copy_slice_to_vec(out, &video_ram);
                Ok(region)
            }
            (ActiveCore::Nes(emu), MemoryRegionKind::PaletteRam) => {
                copy_slice_to_vec(out, emu.ppu_palette_ram());
                Ok(region)
            }
            (ActiveCore::Nes(emu), MemoryRegionKind::Oam) => {
                copy_slice_to_vec(out, emu.ppu_oam());
                Ok(region)
            }
            (ActiveCore::Nes(emu), MemoryRegionKind::SaveRam) => {
                copy_optional_region(out, emu.dump_battery_sram(), region.id)?;
                Ok(region)
            }
            (ActiveCore::Pce(host), MemoryRegionKind::SystemRam) => {
                copy_slice_to_vec(out, host.machine().mapped_work_ram());
                Ok(region)
            }
            (ActiveCore::Pce(host), MemoryRegionKind::VideoRam) => {
                out.clear();
                append_words_le(out, host.machine().devices().vdc().vram());
                if let Some(video) = host.machine().devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().vram());
                }
                Ok(region)
            }
            (ActiveCore::Pce(host), MemoryRegionKind::PaletteRam) => {
                out.clear();
                for color in host.machine().devices().vce().palette() {
                    out.extend_from_slice(&color.raw().to_le_bytes());
                }
                Ok(region)
            }
            (ActiveCore::Pce(host), MemoryRegionKind::Oam) => {
                out.clear();
                append_words_le(out, host.machine().devices().vdc().satb());
                if let Some(video) = host.machine().devices().supergrafx_video() {
                    append_words_le(out, video.vdc2().satb());
                }
                Ok(region)
            }
            (ActiveCore::Pce(host), MemoryRegionKind::SaveRam) => {
                let ram = host.machine().hucard_ram().ok_or_else(|| {
                    anyhow::anyhow!("memory region '{}' is unavailable", region.id)
                })?;
                copy_slice_to_vec(out, ram);
                Ok(region)
            }
            (ActiveCore::Sega8(emu), MemoryRegionKind::SystemRam) => {
                copy_slice_to_vec(out, emu.system_ram());
                Ok(region)
            }
            (ActiveCore::Sega8(emu), MemoryRegionKind::VideoRam) => {
                copy_slice_to_vec(out, emu.video_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Sega8(emu), MemoryRegionKind::PaletteRam) => {
                copy_slice_to_vec(out, emu.palette_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Sega8(emu), MemoryRegionKind::SaveRam) => {
                copy_slice_to_vec(out, emu.bus().cartridge_ram_visible());
                Ok(region)
            }
            (ActiveCore::Ws(emu), MemoryRegionKind::SystemRam) => {
                copy_slice_to_vec(out, emu.system_ram());
                Ok(region)
            }
            (ActiveCore::Ws(emu), MemoryRegionKind::VideoRam) => {
                copy_slice_to_vec(out, emu.video_ram_snapshot());
                Ok(region)
            }
            (ActiveCore::Ws(emu), MemoryRegionKind::SaveRam) => {
                copy_optional_region(out, emu.dump_battery_sram(), region.id)?;
                Ok(region)
            }
            (core, MemoryRegionKind::Framebuffer) => {
                let framebuffer = match core {
                    ActiveCore::Gb(emu) => emu.framebuffer(),
                    ActiveCore::Gba(emu) => emu.framebuffer(),
                    ActiveCore::Nes(emu) => emu.framebuffer(),
                    ActiveCore::Pce(host) => host.framebuffer(),
                    ActiveCore::Sega8(emu) => emu.framebuffer(),
                    ActiveCore::Ws(emu) => emu.framebuffer(),
                };
                copy_slice_to_vec(out, framebuffer);
                Ok(region)
            }
            _ => Err(anyhow::anyhow!(
                "memory region '{}' is not copyable for this libretro core",
                region.id
            )),
        }
    }

    pub fn refresh_system_ram(&mut self) {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let wram = emu.system_ram();
                self.system_ram_buf.resize(wram.len(), 0);
                self.system_ram_buf.copy_from_slice(wram);
            }
            ActiveCore::Gba(emu) => {
                let (ewram, iwram) = emu.system_ram();
                self.system_ram_buf.clear();
                self.system_ram_buf.extend_from_slice(ewram);
                self.system_ram_buf.extend_from_slice(iwram);
            }
            ActiveCore::Nes(emu) => {
                let ram = emu.system_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
            ActiveCore::Pce(host) => {
                let ram = host.machine().mapped_work_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
            ActiveCore::Sega8(emu) => {
                let ram = emu.system_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
            ActiveCore::Ws(emu) => {
                let ram = emu.system_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
        }
    }

    pub fn refresh_video_ram(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                let vram = emu.video_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Gba(emu) => {
                let vram = emu.video_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Nes(emu) => {
                let vram = emu.video_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(&vram);
            }
            ActiveCore::Pce(host) => {
                self.video_ram_buf.clear();
                append_words_le(
                    &mut self.video_ram_buf,
                    host.machine().devices().vdc().vram(),
                );
                if let Some(video) = host.machine().devices().supergrafx_video() {
                    append_words_le(&mut self.video_ram_buf, video.vdc2().vram());
                }
            }
            ActiveCore::Sega8(emu) => {
                let vram = emu.video_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Ws(emu) => {
                let vram = emu.video_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
        }
    }

    pub fn cpu_address_bits(&self) -> u8 {
        match &self.core {
            ActiveCore::Gba(_) => 32,
            ActiveCore::Ws(_) => 20,
            ActiveCore::Gb(_) | ActiveCore::Nes(_) | ActiveCore::Pce(_) | ActiveCore::Sega8(_) => {
                16
            }
        }
    }

    pub fn framebuffer_len(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer().len(),
            ActiveCore::Gba(emu) => emu.framebuffer().len(),
            ActiveCore::Nes(emu) => emu.framebuffer().len(),
            ActiveCore::Pce(host) => host.framebuffer().len(),
            ActiveCore::Sega8(emu) => emu.framebuffer().len(),
            ActiveCore::Ws(emu) => emu.framebuffer().len(),
        }
    }

    pub fn system_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.system_ram().len(),
            ActiveCore::Gba(_) => zeff_gba_core::hardware::constants::SYSTEM_RAM_SIZE,
            ActiveCore::Nes(_) => zeff_nes_core::hardware::constants::SYSTEM_RAM_SIZE,
            ActiveCore::Pce(host) => host.machine().mapped_work_ram().len(),
            ActiveCore::Sega8(emu) => emu.system_ram().len(),
            ActiveCore::Ws(emu) => emu.system_ram().len(),
        }
    }

    pub fn external_work_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.system_ram().0.len(),
            ActiveCore::Gb(_)
            | ActiveCore::Nes(_)
            | ActiveCore::Pce(_)
            | ActiveCore::Sega8(_)
            | ActiveCore::Ws(_) => 0,
        }
    }

    pub fn internal_work_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.system_ram().1.len(),
            ActiveCore::Gb(_)
            | ActiveCore::Nes(_)
            | ActiveCore::Pce(_)
            | ActiveCore::Sega8(_)
            | ActiveCore::Ws(_) => 0,
        }
    }

    pub fn video_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Gba(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Nes(_) => 0x2000,
            ActiveCore::Pce(host) => {
                let multiplier =
                    usize::from(host.machine().devices().supergrafx_video().is_some()) + 1;
                zeff_pce_core::hardware::VDC_VRAM_BYTES * multiplier
            }
            ActiveCore::Sega8(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Ws(emu) => emu.video_ram_snapshot().len(),
        }
    }

    pub fn palette_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.palette_ram_snapshot().len(),
            ActiveCore::Nes(emu) => emu.ppu_palette_ram().len(),
            ActiveCore::Pce(_) => zeff_pce_core::hardware::VCE_PALETTE_COLORS * 2,
            ActiveCore::Sega8(emu) => match emu.system() {
                zeff_sega8_core::hardware::cartridge::Sega8System::MasterSystem
                | zeff_sega8_core::hardware::cartridge::Sega8System::GameGear => {
                    emu.palette_ram_snapshot().len()
                }
                zeff_sega8_core::hardware::cartridge::Sega8System::Sg1000 => 0,
            },
            ActiveCore::Gb(_) | ActiveCore::Ws(_) => 0,
        }
    }

    pub fn oam_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.oam_snapshot().len(),
            ActiveCore::Nes(emu) => emu.ppu_oam().len(),
            ActiveCore::Pce(host) => {
                let multiplier =
                    usize::from(host.machine().devices().supergrafx_video().is_some()) + 1;
                zeff_pce_core::hardware::VDC_SATB_WORDS * 2 * multiplier
            }
            ActiveCore::Gb(_) | ActiveCore::Sega8(_) | ActiveCore::Ws(_) => 0,
        }
    }

    pub fn io_registers_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.io_snapshot().len(),
            ActiveCore::Gb(_)
            | ActiveCore::Nes(_)
            | ActiveCore::Pce(_)
            | ActiveCore::Sega8(_)
            | ActiveCore::Ws(_) => 0,
        }
    }
}

#[allow(dead_code)]
fn copy_slice_to_vec(out: &mut Vec<u8>, data: &[u8]) {
    out.clear();
    out.extend_from_slice(data);
}

fn append_words_le(out: &mut Vec<u8>, words: &[u16]) {
    out.reserve(words.len() * 2);
    for word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
}

#[allow(dead_code)]
fn copy_optional_region(
    out: &mut Vec<u8>,
    data: Option<Vec<u8>>,
    region_id: &str,
) -> anyhow::Result<()> {
    let data = data.ok_or_else(|| anyhow::anyhow!("memory region '{region_id}' is unavailable"))?;
    copy_slice_to_vec(out, &data);
    Ok(())
}
