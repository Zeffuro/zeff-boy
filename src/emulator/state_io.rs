use super::Emulator;
use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::bus::Bus;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::rom_loader;
use crate::save_state::{
    SAVE_STATE_MAGIC, SAVE_STATE_VERSION, SaveStateRef, has_bess_footer, import_bess, slot_path,
    validate_compatibility, write_to_file,
};
use anyhow::{Result as AnyResult, bail};
use std::path::Path;

impl Emulator {
    pub(crate) fn flush_battery_sram(&self) -> AnyResult<Option<String>> {
        if !self.header.cartridge_type.is_battery_backed() {
            return Ok(None);
        }

        let sram = self.bus.cartridge.dump_sram();
        if sram.is_empty() {
            return Ok(None);
        }

        let save_path = rom_loader::save_file_path_for_rom(&self.rom_path);
        rom_loader::write_save_file(&save_path, &sram)?;
        Ok(Some(save_path.display().to_string()))
    }

    pub(super) fn try_load_battery_sram(&mut self) -> AnyResult<Option<String>> {
        if !self.header.cartridge_type.is_battery_backed() {
            return Ok(None);
        }

        let expected_len = self.bus.cartridge.sram_len();
        let has_mbc3_rtc = self.header.cartridge_type.is_mbc3_with_rtc();
        if expected_len == 0 && !has_mbc3_rtc {
            return Ok(None);
        }

        let save_path = rom_loader::save_file_path_for_rom(&self.rom_path);
        if !save_path.exists() {
            return Ok(None);
        }

        let loaded = rom_loader::load_save_file(&save_path)?;
        if has_mbc3_rtc {
            let legacy_len = expected_len.saturating_sub(4);
            if loaded.len() != expected_len && loaded.len() != legacy_len {
                log::warn!(
                    "SRAM/RTC size mismatch for {}: got {} bytes, expected {} or {} (will truncate/pad RAM, RTC footer ignored)",
                    save_path.display(),
                    loaded.len(),
                    legacy_len,
                    expected_len
                );
            }
            self.bus.cartridge.load_sram(&loaded);
            return Ok(Some(save_path.display().to_string()));
        }

        if loaded.len() != expected_len {
            log::warn!(
                "SRAM size mismatch for {}: got {} bytes, expected {} (will truncate/pad)",
                save_path.display(),
                loaded.len(),
                expected_len
            );
        }

        let mut adjusted = vec![0u8; expected_len];
        let copy_len = expected_len.min(loaded.len());
        adjusted[..copy_len].copy_from_slice(&loaded[..copy_len]);
        self.bus.cartridge.load_sram(&adjusted);

        Ok(Some(save_path.display().to_string()))
    }

    pub(crate) fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    pub(crate) fn as_save_state_ref(&self) -> SaveStateRef<'_> {
        SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: self.rom_hash,
            cpu: &self.cpu,
            bus: self.bus.as_ref(),
            hardware_mode_preference: self.hardware_mode_preference,
            hardware_mode: self.hardware_mode,
            cycle_count: self.cycle_count,
            last_opcode: self.last_opcode,
            last_opcode_pc: self.last_opcode_pc,
        }
    }

    pub(crate) fn encode_state_bytes(&self) -> AnyResult<Vec<u8>> {
        crate::save_state::encode_state_bytes(&self.as_save_state_ref())
    }

    #[allow(dead_code)]
    pub(crate) fn save_state(&self, slot: u8) -> AnyResult<String> {
        let path = slot_path(self.rom_hash, slot)?;
        self.save_state_to_path(&path)?;
        Ok(path.display().to_string())
    }

    #[allow(dead_code)]
    pub(crate) fn save_state_to_path(&self, path: &Path) -> AnyResult<()> {
        write_to_file(path, &self.as_save_state_ref())?;
        Ok(())
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> AnyResult<String> {
        let path = slot_path(self.rom_hash, slot)?;
        self.load_state_from_path(&path)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> AnyResult<()> {
        let bytes = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("failed to read save state: {}: {e}", path.display()))?;
        self.load_state_from_bytes(bytes)
    }

    pub(crate) fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> AnyResult<()> {
        if bytes.len() >= 8 && bytes[..8] == SAVE_STATE_MAGIC {
            let state = crate::save_state::decode_on_thread(bytes)?;
            validate_compatibility(&state, self.rom_hash)?;

            let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
            let mut restored_bus = state.bus;
            restored_bus.cartridge.restore_rom_bytes(rom_bytes);
            Self::apply_bus_fixups(
                &mut restored_bus,
                self.bus.io.apu.sample_rate,
                self.header.cartridge_type.is_mbc5_with_rumble(),
            );

            self.cpu = state.cpu;
            *self.bus = restored_bus;
            self.hardware_mode_preference = state.hardware_mode_preference;
            self.hardware_mode = state.hardware_mode;
            self.cycle_count = state.cycle_count;
            self.last_opcode = state.last_opcode;
            self.last_opcode_pc = state.last_opcode_pc;

            self.reset_debug_state();
            return Ok(());
        }

        if has_bess_footer(&bytes) {
            let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
            let import = import_bess(&bytes, &rom_bytes, &self.header)?;

            let mut restored_bus = import.bus;
            Self::apply_bus_fixups(
                &mut restored_bus,
                self.bus.io.apu.sample_rate,
                self.header.cartridge_type.is_mbc5_with_rumble(),
            );

            self.cpu = import.cpu;
            self.bus = restored_bus;
            self.hardware_mode = import.hardware_mode;
            self.cycle_count = 0;
            self.last_opcode = 0;
            self.last_opcode_pc = self.cpu.pc;

            self.reset_debug_state();
            return Ok(());
        }

        bail!("unrecognized save state format")
    }

    /// Apply common fixups to a restored `Bus` (rumble, timer/serial mode, sample rate, SGB).
    fn apply_bus_fixups(bus: &mut Bus, current_sample_rate: u32, is_rumble: bool) {
        bus.cartridge.set_rumble_flag(is_rumble);
        bus.io.timer.mode = bus.hardware_mode;
        bus.io.serial.mode = bus.hardware_mode;
        bus.io.apu.set_sample_rate(current_sample_rate);
        bus.io.ppu.set_sgb_mode(matches!(
            bus.hardware_mode,
            HardwareMode::SGB1 | HardwareMode::SGB2
        ));
    }

    /// Reset debug/opcode state after loading a save state.
    fn reset_debug_state(&mut self) {
        self.opcode_log = OpcodeLog::new(32);
        self.debug = DebugController::new();
        self.bus.trace_cpu_accesses = false;
        self.bus.begin_cpu_access_trace();
    }
}
