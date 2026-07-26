use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_gb_core::emulator::Emulator as GbEmulator;

use crate::audio_recorder::MidiApuSnapshot;
use crate::emu_core_trait::EmulatorCore;

impl crate::emu_core_trait::DebuggableEmulator for GbEmulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(narrow_u16(addr))
    }
    fn add_watchpoint(&mut self, addr: Address, wt: zeff_emu_common::debug::WatchType) {
        self.add_watchpoint(narrow_u16(addr), wt)
    }
    fn remove_breakpoint(&mut self, addr: Address) {
        self.remove_breakpoint(narrow_u16(addr))
    }
    fn toggle_breakpoint(&mut self, addr: Address) {
        self.toggle_breakpoint(narrow_u16(addr))
    }
    fn debug_write(&mut self, addr: Address, val: u8) {
        self.write_byte(narrow_u16(addr), val)
    }
}

pub(crate) struct GbBackend {
    pub(crate) emu: GbEmulator,
    rom_path: PathBuf,
    source_path: PathBuf,
}

impl GbBackend {
    pub(crate) fn new(emu: GbEmulator, rom_path: PathBuf) -> Self {
        Self::with_source_path(emu, rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(
        emu: GbEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self {
            emu,
            rom_path,
            source_path,
        }
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }
}

impl EmulatorCore for GbBackend {
    #[inline]
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    #[inline]
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    #[inline]
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    #[inline]
    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr: [bool; 4] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(&self.rom_path, self.emu.dump_battery_sram())
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state_bytes()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.emu.rom_hash()
    }

    #[inline]
    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Gb(self.emu.apu_channel_snapshot()))
    }

    #[inline]
    fn rumble_active(&self) -> bool {
        self.emu.rumble_active()
    }

    #[inline]
    fn is_mbc7(&self) -> bool {
        self.emu.is_mbc7_cartridge()
    }

    #[inline]
    fn is_pocket_camera(&self) -> bool {
        self.emu.is_pocket_camera_cartridge()
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut GbEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "GB", emu.is_battery_backed(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
