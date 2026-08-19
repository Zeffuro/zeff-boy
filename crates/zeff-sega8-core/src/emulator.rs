use std::path::Path;

use sha2::{Digest, Sha256};
use zeff_emu_common::debug::{AddressDebugController, OpcodeLog};

use crate::hardware::bus::Bus;
use crate::hardware::cartridge::{Cartridge, Sega8MapperKind, Sega8System, SystemHint};
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, RGBA_CHANNELS, SMS_SCREEN_H, SMS_SCREEN_W,
};
use crate::hardware::cpu::Cpu;
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;

mod public_api;
mod runtime;
mod state_io;

#[cfg(test)]
mod tests;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const HOST_DPAD_RIGHT: u8 = 1 << 0;
const HOST_DPAD_LEFT: u8 = 1 << 1;
const HOST_DPAD_UP: u8 = 1 << 2;
const HOST_DPAD_DOWN: u8 = 1 << 3;
const HOST_BUTTON_1: u8 = 1 << 0;
const HOST_BUTTON_2: u8 = 1 << 1;
const HOST_BUTTON_START: u8 = 1 << 3;
const SMS_PAD_UP: u8 = 1 << 0;
const SMS_PAD_DOWN: u8 = 1 << 1;
const SMS_PAD_LEFT: u8 = 1 << 2;
const SMS_PAD_RIGHT: u8 = 1 << 3;
const SMS_PAD_BUTTON_1: u8 = 1 << 4;
const SMS_PAD_BUTTON_2: u8 = 1 << 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sega8LoadConfig {
    pub sample_rate: u32,
    pub system_hint: SystemHint,
    pub mapper_kind: Option<Sega8MapperKind>,
    pub video_standard: Option<Sega8VideoStandard>,
    pub console_region: Option<Sega8Region>,
    pub console_region_fallback: Option<Sega8Region>,
}

impl Default for Sega8LoadConfig {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            system_hint: SystemHint::Auto,
            mapper_kind: None,
            video_standard: None,
            console_region: None,
            console_region_fallback: None,
        }
    }
}

impl Sega8LoadConfig {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ..Self::default()
        }
    }

    pub fn from_path(sample_rate: u32, path: &Path) -> Self {
        Self {
            sample_rate,
            system_hint: SystemHint::from_path(path).unwrap_or_default(),
            mapper_kind: Sega8MapperKind::from_path(path),
            video_standard: Sega8VideoStandard::from_path(path),
            console_region_fallback: Sega8Region::from_path(path),
            ..Self::default()
        }
    }

    pub fn with_system_hint(mut self, system_hint: SystemHint) -> Self {
        self.system_hint = system_hint;
        self
    }

    pub fn with_mapper_kind(mut self, mapper_kind: Option<Sega8MapperKind>) -> Self {
        self.mapper_kind = mapper_kind;
        self
    }

    pub fn with_video_standard(mut self, video_standard: Sega8VideoStandard) -> Self {
        self.video_standard = Some(video_standard);
        self
    }

    pub fn with_console_region(mut self, console_region: Option<Sega8Region>) -> Self {
        self.console_region = console_region;
        self
    }

    pub fn with_console_region_fallback(
        mut self,
        console_region_fallback: Option<Sega8Region>,
    ) -> Self {
        self.console_region_fallback = console_region_fallback;
        self
    }
}

#[derive(Debug)]
pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) frame_count: u64,
    pub(crate) framebuffer: Vec<u8>,
    pub(crate) sample_rate: u32,
    pub(crate) video_standard: Sega8VideoStandard,
    pub(crate) console_region: Sega8Region,
    pub(crate) debug: AddressDebugController,
    pub(crate) opcode_log: OpcodeLog<(u16, u8, u32)>,
    pub(crate) instruction_trace: zeff_emu_common::debug::InstructionTraceStore,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        Self::new_with_config(rom_data, Sega8LoadConfig::new(sample_rate))
    }

    pub fn new_with_path_hint(
        rom_data: &[u8],
        sample_rate: u32,
        path: &Path,
    ) -> anyhow::Result<Self> {
        Self::new_with_config(rom_data, Sega8LoadConfig::from_path(sample_rate, path))
    }

    pub fn new_with_hint(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
    ) -> anyhow::Result<Self> {
        Self::new_with_config(
            rom_data,
            Sega8LoadConfig::new(sample_rate).with_system_hint(hint),
        )
    }

    pub fn new_with_hint_and_video_standard(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
        video_standard: Sega8VideoStandard,
    ) -> anyhow::Result<Self> {
        Self::new_with_config(
            rom_data,
            Sega8LoadConfig::new(sample_rate)
                .with_system_hint(hint)
                .with_video_standard(video_standard),
        )
    }

    pub fn new_with_hint_video_standard_and_region(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
        video_standard: Sega8VideoStandard,
        console_region: Option<Sega8Region>,
    ) -> anyhow::Result<Self> {
        Self::new_with_config(
            rom_data,
            Sega8LoadConfig::new(sample_rate)
                .with_system_hint(hint)
                .with_video_standard(video_standard)
                .with_console_region(console_region),
        )
    }

    pub fn new_with_hint_video_standard_region_fallback(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
        video_standard: Sega8VideoStandard,
        console_region: Option<Sega8Region>,
        console_region_fallback: Option<Sega8Region>,
    ) -> anyhow::Result<Self> {
        Self::new_with_config(
            rom_data,
            Sega8LoadConfig::new(sample_rate)
                .with_system_hint(hint)
                .with_video_standard(video_standard)
                .with_console_region(console_region)
                .with_console_region_fallback(console_region_fallback),
        )
    }

    pub fn new_with_config(rom_data: &[u8], config: Sega8LoadConfig) -> anyhow::Result<Self> {
        Self::new_with_config_inner(rom_data, config, None)
    }

    pub fn new_with_config_and_boot_rom(
        rom_data: &[u8],
        config: Sega8LoadConfig,
        boot_rom: &[u8],
    ) -> anyhow::Result<Self> {
        Self::new_with_config_inner(rom_data, config, Some(boot_rom))
    }

    fn new_with_config_inner(
        rom_data: &[u8],
        config: Sega8LoadConfig,
        boot_rom: Option<&[u8]>,
    ) -> anyhow::Result<Self> {
        let sample_rate = if config.sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            config.sample_rate
        };
        let video_standard = config.video_standard.unwrap_or_default();
        let cartridge = Cartridge::load_with_hint_and_mapper_kind(
            rom_data,
            config.system_hint,
            config.mapper_kind,
        )?;
        let console_region = config
            .console_region
            .or_else(|| {
                cartridge
                    .header()
                    .and_then(|header| Sega8Region::from_header_region(header.region))
            })
            .or(config.console_region_fallback)
            .unwrap_or_default();
        if let Some(boot_rom) = boot_rom {
            let expected_len = match cartridge.system() {
                Sega8System::MasterSystem => 0x2000,
                Sega8System::GameGear => 0x0400,
                Sega8System::Sg1000 => anyhow::bail!("SG-1000 does not use a boot ROM"),
            };
            anyhow::ensure!(
                boot_rom.len() == expected_len,
                "{:?} boot ROM must be {expected_len} bytes, got {}",
                cartridge.system(),
                boot_rom.len()
            );
        }
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let framebuffer = vec![0; framebuffer_len(cartridge.system())];
        let mut bus = Bus::new_with_sample_rate_video_standard_and_region(
            cartridge,
            sample_rate,
            video_standard,
            console_region,
        );
        bus.set_boot_rom(boot_rom.map(<[u8]>::to_vec));
        Ok(Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            frame_count: 0,
            framebuffer,
            sample_rate,
            video_standard,
            console_region,
            debug: AddressDebugController::new(),
            opcode_log: OpcodeLog::new(),
            instruction_trace: zeff_emu_common::debug::InstructionTraceStore::default(),
        })
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
        self.frame_count = 0;
        self.framebuffer.fill(0);
        self.debug.clear_hits();
        self.opcode_log.clear();
        self.instruction_trace.clear();
    }
}

fn dimensions_for_system(system: Sega8System) -> (usize, usize) {
    match system {
        Sega8System::GameGear => (GG_SCREEN_W, GG_SCREEN_H),
        Sega8System::MasterSystem | Sega8System::Sg1000 => (SMS_SCREEN_W, SMS_SCREEN_H),
    }
}

fn framebuffer_len(system: Sega8System) -> usize {
    let (w, h) = dimensions_for_system(system);
    w * h * RGBA_CHANNELS
}
