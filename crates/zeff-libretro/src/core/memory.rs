use super::{ActiveCore, CoreState};
use zeff_emu_common::memory::{
    ExtendedMemoryRegionSizes, MemoryRegionDescriptor, MemoryRegionKind,
    standard_memory_regions_with_extended,
};
use zeff_emu_common::save_ram::SaveRamKind;

impl CoreState {
    pub fn battery_sram(&self) -> Option<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.dump_battery_sram(),
            ActiveCore::Gba(emu) => emu.dump_battery_sram(),
            ActiveCore::Nes(emu) => emu.dump_battery_sram(),
            ActiveCore::Sega8(emu) => emu.dump_battery_sram(),
            ActiveCore::Ws(emu) => emu.dump_battery_sram(),
        }
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        match &self.core {
            ActiveCore::Gb(emu) => emu.save_ram_kind(),
            ActiveCore::Gba(emu) => emu.save_ram_kind(),
            ActiveCore::Nes(emu) => emu.save_ram_kind(),
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
            ActiveCore::Gb(_) | ActiveCore::Nes(_) | ActiveCore::Sega8(_) => 16,
        }
    }

    pub fn framebuffer_len(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer().len(),
            ActiveCore::Gba(emu) => emu.framebuffer().len(),
            ActiveCore::Nes(emu) => emu.framebuffer().len(),
            ActiveCore::Sega8(emu) => emu.framebuffer().len(),
            ActiveCore::Ws(emu) => emu.framebuffer().len(),
        }
    }

    pub fn system_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.system_ram().len(),
            ActiveCore::Gba(_) => 0x48000,
            ActiveCore::Nes(_) => 0x800,
            ActiveCore::Sega8(emu) => emu.system_ram().len(),
            ActiveCore::Ws(emu) => emu.system_ram().len(),
        }
    }

    pub fn video_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Gba(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Nes(_) => 0x2000,
            ActiveCore::Sega8(emu) => emu.video_ram_snapshot().len(),
            ActiveCore::Ws(emu) => emu.video_ram_snapshot().len(),
        }
    }

    pub fn palette_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.palette_ram_snapshot().len(),
            ActiveCore::Nes(emu) => emu.ppu_palette_ram().len(),
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
            ActiveCore::Gb(_) | ActiveCore::Sega8(_) | ActiveCore::Ws(_) => 0,
        }
    }

    pub fn io_registers_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gba(emu) => emu.io_snapshot().len(),
            ActiveCore::Gb(_) | ActiveCore::Nes(_) | ActiveCore::Sega8(_) | ActiveCore::Ws(_) => 0,
        }
    }
}
