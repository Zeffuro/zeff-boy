use std::path::Path;

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_nes_core::emulator::Emulator as NesEmulator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveSystem {
    GameBoy,
    Nes,
}

impl ActiveSystem {
    pub(crate) fn screen_size(self) -> (u32, u32) {
        match self {
            Self::GameBoy => (160, 144),
            Self::Nes => (256, 240),
        }
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "gb" | "gbc" => Some(Self::GameBoy),
            "nes" => Some(Self::Nes),
            _ => None,
        }
    }
}

pub(crate) enum EmuBackend {
    Gb(GbEmulator),
    Nes(NesEmulator),
}

impl EmuBackend {
    pub(crate) fn system(&self) -> ActiveSystem {
        match self {
            Self::Gb(_) => ActiveSystem::GameBoy,
            Self::Nes(_) => ActiveSystem::Nes,
        }
    }

    pub(crate) fn screen_size(&self) -> (u32, u32) {
        self.system().screen_size()
    }

    pub(crate) fn from_gb(emu: GbEmulator) -> Self {
        Self::Gb(emu)
    }

    pub(crate) fn from_nes(emu: NesEmulator) -> Self {
        Self::Nes(emu)
    }

    pub(crate) fn gb(&self) -> Option<&GbEmulator> {
        match self {
            Self::Gb(e) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn gb_mut(&mut self) -> Option<&mut GbEmulator> {
        match self {
            Self::Gb(e) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn nes_mut(&mut self) -> Option<&mut NesEmulator> {
        match self {
            Self::Nes(e) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn step_frame(&mut self) {
        match self {
            Self::Gb(e) => e.step_frame(),
            Self::Nes(e) => e.step_frame(),
        }
    }

    pub(crate) fn framebuffer(&self) -> &[u8] {
        match self {
            Self::Gb(e) => e.framebuffer(),
            Self::Nes(e) => e.framebuffer(),
        }
    }

    pub(crate) fn drain_audio_samples(&mut self) -> Vec<f32> {
        match self {
            Self::Gb(e) => e.bus.apu_drain_samples(),
            Self::Nes(e) => {
                let mono = e.drain_audio_samples();
                let mut stereo = Vec::with_capacity(mono.len() * 2);
                for &s in &mono {
                    stereo.push(s);
                    stereo.push(s);
                }
                stereo
            }
        }
    }

    pub(crate) fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        match self {
            Self::Gb(e) => e.bus.apu_drain_samples_into(buf),
            Self::Nes(e) => {
                let mono = e.drain_audio_samples();
                buf.clear();
                buf.reserve(mono.len() * 2);
                for &s in &mono {
                    buf.push(s);
                    buf.push(s);
                }
            }
        }
    }

    pub(crate) fn set_sample_rate(&mut self, rate: u32) {
        match self {
            Self::Gb(e) => e.bus.set_apu_sample_rate(rate),
            Self::Nes(e) => e.bus.apu.output_sample_rate = rate as f64,
        }
    }

    pub(crate) fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        match self {
            Self::Gb(e) => e.bus.set_apu_sample_generation_enabled(enabled),
            Self::Nes(_) => { /* NES APU always generates samples */ }
        }
    }

    pub(crate) fn rom_path(&self) -> &Path {
        match self {
            Self::Gb(e) => e.rom_path(),
            Self::Nes(e) => &e.rom_path,
        }
    }

    pub(crate) fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        match self {
            Self::Gb(e) => {
                if e.bus.apply_joypad_pressed_masks(buttons_pressed, dpad_pressed) {
                    e.bus.if_reg |= 0x10;
                }
            }
            Self::Nes(e) => {
                let mut nes_byte = 0u8;
                if buttons_pressed & 0x01 != 0 { nes_byte |= 0x01; } // A
                if buttons_pressed & 0x02 != 0 { nes_byte |= 0x02; } // B
                if buttons_pressed & 0x04 != 0 { nes_byte |= 0x04; } // Select
                if buttons_pressed & 0x08 != 0 { nes_byte |= 0x08; } // Start
                if dpad_pressed & 0x04 != 0 { nes_byte |= 0x10; }    // Up
                if dpad_pressed & 0x08 != 0 { nes_byte |= 0x20; }    // Down
                if dpad_pressed & 0x02 != 0 { nes_byte |= 0x40; }    // Left
                if dpad_pressed & 0x01 != 0 { nes_byte |= 0x80; }    // Right
                e.bus.controller1.set_buttons(nes_byte);
            }
        }
    }

    pub(crate) fn is_suspended(&self) -> bool {
        match self {
            Self::Gb(e) => matches!(e.cpu.running, zeff_gb_core::hardware::types::CPUState::Suspended),
            Self::Nes(e) => matches!(e.cpu.state, zeff_nes_core::hardware::cpu::CpuState::Suspended),
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.is_suspended()
    }

    pub(crate) fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        match self {
            Self::Gb(e) => e.flush_battery_sram(),
            Self::Nes(_) => Ok(None),
        }
    }

    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match self {
            Self::Gb(e) => e.encode_state_bytes(),
            Self::Nes(_) => Err(anyhow::anyhow!("NES save states not yet supported")),
        }
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        match self {
            Self::Gb(e) => e.load_state_from_bytes(bytes),
            Self::Nes(_) => Err(anyhow::anyhow!("NES save states not yet supported")),
        }
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> anyhow::Result<String> {
        match self {
            Self::Gb(e) => e.load_state(slot),
            Self::Nes(_) => Err(anyhow::anyhow!("NES save states not yet supported")),
        }
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Gb(e) => e.load_state_from_path(path),
            Self::Nes(_) => Err(anyhow::anyhow!("NES save states not yet supported")),
        }
    }

    pub(crate) fn rom_hash(&self) -> Option<[u8; 32]> {
        match self {
            Self::Gb(e) => Some(e.rom_hash),
            Self::Nes(_) => None,
        }
    }

    pub(crate) fn rumble_active(&self) -> bool {
        match self {
            Self::Gb(e) => e.bus.cartridge.rumble_active(),
            Self::Nes(_) => false,
        }
    }

    pub(crate) fn is_mbc7(&self) -> bool {
        match self {
            Self::Gb(e) => e.is_mbc7_cartridge(),
            Self::Nes(_) => false,
        }
    }

    pub(crate) fn apu_channel_snapshot(&self) -> Option<zeff_gb_core::hardware::apu::ApuChannelSnapshot> {
        match self {
            Self::Gb(e) => Some(e.bus.apu_channel_snapshot()),
            Self::Nes(_) => None,
        }
    }
}

