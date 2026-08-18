use super::Emulator;
use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::bus::Bus;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{
    SAVE_STATE_MAGIC, SAVE_STATE_VERSION, SaveStateRef, has_bess_footer, import_bess,
    validate_compatibility,
};
use anyhow::{Result as AnyResult, bail};
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn is_battery_backed(&self) -> bool {
        self.header.cartridge_type.is_battery_backed()
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        if !self.is_battery_backed() {
            return SaveRamKind::none();
        }

        let size = self.bus.cartridge.sram_len();
        if size == 0 {
            SaveRamKind::none()
        } else {
            SaveRamKind::known_battery_backed(size)
        }
    }

    pub fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        if !self.has_battery() {
            return None;
        }

        let sram = self.bus.cartridge.dump_sram();
        if sram.is_empty() {
            return None;
        }

        Some(sram)
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> AnyResult<()> {
        let expected_len = self.bus.cartridge.sram_len();
        let has_mbc3_rtc = self.header.cartridge_type.is_mbc3_with_rtc();
        if expected_len == 0 && !has_mbc3_rtc {
            return Ok(());
        }

        if has_mbc3_rtc {
            self.bus.cartridge.load_sram(bytes);
            return Ok(());
        }

        let mut adjusted = vec![0u8; expected_len];
        let copy_len = expected_len.min(bytes.len());
        adjusted[..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.bus.cartridge.load_sram(&adjusted);

        Ok(())
    }

    pub fn as_save_state_ref(&self) -> SaveStateRef<'_> {
        SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: self.rom_hash,
            cpu: &self.cpu,
            bus: &self.bus,
            hardware_mode_preference: self.hardware_mode_preference,
            hardware_mode: self.hardware_mode,
            cycle_count: self.cycle_count,
            last_opcode: self.last_opcode,
            last_opcode_pc: self.last_opcode_pc,
        }
    }

    pub fn encode_state_bytes(&self) -> AnyResult<Vec<u8>> {
        crate::save_state::encode_state_bytes(&self.as_save_state_ref())
    }

    pub fn encode_state(&self) -> AnyResult<Vec<u8>> {
        self.encode_state_bytes()
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> AnyResult<()> {
        self.load_state_from_bytes(bytes.to_vec())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> AnyResult<()> {
        if bytes.len() >= 8 && bytes[..8] == SAVE_STATE_MAGIC {
            match crate::save_state::decode_on_thread(bytes.clone()) {
                Ok(state) => {
                    validate_compatibility(&state, self.rom_hash)?;

                    let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
                    let mut restored_bus = state.bus;
                    restored_bus.restore_cartridge_rom_bytes(rom_bytes, &self.header);
                    Self::apply_bus_fixups(
                        &mut restored_bus,
                        self.bus.apu_sample_rate(),
                        self.header.cartridge_type.is_mbc5_with_rumble(),
                    );

                    self.cpu = state.cpu;
                    *self.bus = restored_bus;
                    self.hardware_mode_preference = state.hardware_mode_preference;
                    self.hardware_mode = state.hardware_mode;
                    self.cycle_count = state.cycle_count;
                    self.frame_count = self.cpu.cycles / Self::cycles_per_frame(self.hardware_mode);
                    self.last_opcode = state.last_opcode;
                    self.last_opcode_pc = state.last_opcode_pc;

                    self.reset_debug_state();
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("native save state decode failed, trying BESS fallback: {e}");
                }
            }
        }

        if has_bess_footer(&bytes) {
            let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
            let import = import_bess(&bytes, &rom_bytes, &self.header)?;

            let mut restored_bus = import.bus;
            Self::apply_bus_fixups(
                &mut restored_bus,
                self.bus.apu_sample_rate(),
                self.header.cartridge_type.is_mbc5_with_rumble(),
            );

            self.cpu = import.cpu;
            *self.bus = restored_bus;
            self.hardware_mode = import.hardware_mode;
            self.cycle_count = 0;
            self.frame_count = self.cpu.cycles / Self::cycles_per_frame(self.hardware_mode);
            self.last_opcode = 0;
            self.last_opcode_pc = self.cpu.pc;

            self.reset_debug_state();
            return Ok(());
        }

        bail!("unrecognized save state format")
    }

    fn apply_bus_fixups(bus: &mut Bus, current_sample_rate: u32, is_rumble: bool) {
        bus.cartridge.set_rumble_flag(is_rumble);
        bus.sync_timer_serial_mode();
        bus.set_apu_sample_rate(current_sample_rate);
        bus.set_ppu_sgb_mode(matches!(
            bus.hardware_mode,
            HardwareMode::SGB1 | HardwareMode::SGB2
        ));
    }

    fn reset_debug_state(&mut self) {
        self.opcode_log = OpcodeLog::new();
        self.instruction_trace.clear();
        self.call_stack.clear();
        self.debug = DebugController::new();
        self.bus.trace_cpu_accesses = false;
        self.bus.begin_cpu_access_trace();
    }
}
