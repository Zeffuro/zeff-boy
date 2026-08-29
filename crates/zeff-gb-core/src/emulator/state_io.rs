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
            boot_rom_enabled: self.bus.boot_rom_enabled(),
            frame_count: self.frame_count,
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
        let ppu_debug_flags = self.bus.ppu_debug_flags();
        if bytes.len() >= 8 && bytes[..8] == SAVE_STATE_MAGIC {
            return match crate::save_state::decode_on_thread(bytes) {
                Ok(state) => {
                    validate_compatibility(&state, self.rom_hash)?;

                    let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
                    let boot_rom = self.bus.boot_rom_bytes().map(<[u8]>::to_vec);
                    let mut restored_bus = state.bus;
                    restored_bus.restore_cartridge_rom_bytes(rom_bytes, &self.header);
                    if state.boot_rom_enabled && boot_rom.is_none() {
                        bail!("save state is still executing the GB boot ROM, but none is loaded");
                    }
                    restored_bus.set_boot_rom(boot_rom, state.boot_rom_enabled);
                    Self::apply_bus_fixups(
                        &mut restored_bus,
                        self.bus.apu_sample_rate(),
                        self.header.cartridge_type.is_mbc5_with_rumble(),
                    );
                    if let Some(framebuffer) = state.lcd_framebuffer {
                        restored_bus.restore_ppu_lcd_framebuffer(framebuffer);
                    }
                    restored_bus.set_ppu_debug_flags(
                        ppu_debug_flags.bg,
                        ppu_debug_flags.window,
                        ppu_debug_flags.sprites,
                    );

                    self.cpu = state.cpu;
                    *self.bus = restored_bus;
                    self.hardware_mode_preference = state.hardware_mode_preference;
                    self.hardware_mode = state.hardware_mode;
                    self.cycle_count = state.cycle_count;
                    self.frame_count = state.frame_count.unwrap_or_else(|| {
                        self.cpu.cycles / Self::cycles_per_frame(self.hardware_mode)
                    });
                    self.last_opcode = state.last_opcode;
                    self.last_opcode_pc = state.last_opcode_pc;

                    self.reset_debug_state();
                    Ok(())
                }
                Err(error) => Err(error),
            };
        }

        if has_bess_footer(&bytes) {
            let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
            let boot_rom = self.bus.boot_rom_bytes().map(<[u8]>::to_vec);
            let import = import_bess(&bytes, &rom_bytes, &self.header)?;

            let mut restored_bus = import.bus;
            restored_bus.set_boot_rom(boot_rom, false);
            Self::apply_bus_fixups(
                &mut restored_bus,
                self.bus.apu_sample_rate(),
                self.header.cartridge_type.is_mbc5_with_rumble(),
            );
            restored_bus.set_ppu_debug_flags(
                ppu_debug_flags.bg,
                ppu_debug_flags.window,
                ppu_debug_flags.sprites,
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

    pub fn probe_native_state_for_replay(
        &self,
        bytes: &[u8],
        link_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
    ) -> AnyResult<(u64, u64)> {
        let state = crate::save_state::decode_on_thread(bytes.to_vec())?;
        validate_compatibility(&state, self.rom_hash)?;
        if state.boot_rom_enabled && self.bus.boot_rom_bytes().is_none() {
            bail!("save state is still executing the GB boot ROM, but none is loaded");
        }
        let mut bus = state.bus;
        bus.restore_cartridge_rom_bytes(self.bus.cartridge.rom_bytes().to_vec(), &self.header);
        bus.set_boot_rom(
            self.bus.boot_rom_bytes().map(<[u8]>::to_vec),
            state.boot_rom_enabled,
        );
        Self::apply_bus_fixups(
            &mut bus,
            self.bus.apu_sample_rate(),
            self.header.cartridge_type.is_mbc5_with_rumble(),
        );
        if let Some(link_state) = link_state
            && !bus.restore_game_boy_link_replay_state(link_state)
        {
            bail!("replay contains an invalid Game Boy link start state");
        }
        Ok((
            state
                .frame_count
                .unwrap_or_else(|| state.cpu.cycles / Self::cycles_per_frame(state.hardware_mode)),
            state.cpu.cycles,
        ))
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

#[cfg(test)]
mod tests {
    use super::Emulator;
    use crate::hardware::ppu::{SCREEN_H, SCREEN_W};
    use crate::hardware::types::CpuState;
    use crate::hardware::types::hardware_mode::HardwareModePreference;

    const LCD_FRAMEBUFFER_LEN: usize = SCREEN_W * SCREEN_H * 4;

    fn test_rom(sgb: bool) -> Vec<u8> {
        let mut rom = vec![0; 0x8000];
        if sgb {
            rom[0x146] = 0x03;
            rom[0x14B] = 0x33;
        }
        rom
    }

    fn emulator(sgb: bool) -> Emulator {
        Emulator::from_rom_data(
            &test_rom(sgb),
            if sgb {
                HardwareModePreference::ForceSgb
            } else {
                HardwareModePreference::ForceDmg
            },
        )
        .unwrap()
    }

    fn patterned_framebuffer(seed: u8) -> Box<[u8]> {
        (0..LCD_FRAMEBUFFER_LEN)
            .map(|index| seed.wrapping_add(index as u8))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn assert_ppu_debug_flags(emu: &Emulator, expected: (bool, bool, bool)) {
        let flags = emu.bus.ppu_debug_flags();
        assert_eq!((flags.bg, flags.window, flags.sprites), expected);
    }

    #[test]
    fn reset_preserves_host_ppu_debug_flags() {
        let mut emu = emulator(false);
        emu.set_ppu_debug_flags(false, true, false);

        emu.reset();

        assert_ppu_debug_flags(&emu, (false, true, false));
    }

    #[test]
    fn public_load_preserves_host_ppu_debug_flags_for_native_and_bess() {
        let source = emulator(false);
        let native = source.encode_state_bytes().unwrap();
        let mut target = emulator(false);
        target.set_ppu_debug_flags(false, true, false);

        target.load_state(&native).unwrap();
        assert_ppu_debug_flags(&target, (false, true, false));

        let mut bess = native;
        bess[0] ^= 0xFF;
        target.set_ppu_debug_flags(true, false, false);
        target.load_state(&bess).unwrap();
        assert_ppu_debug_flags(&target, (true, false, false));
    }

    fn assert_bytes_equal(label: &str, actual: &[u8], expected: &[u8]) {
        assert_eq!(actual.len(), expected.len(), "{label} length");
        let difference = actual
            .iter()
            .zip(expected)
            .position(|(actual, expected)| actual != expected);
        assert!(
            difference.is_none(),
            "{label} first differs at {difference:?}"
        );
    }

    fn zeff_extension_range(bytes: &[u8]) -> std::ops::Range<usize> {
        let footer_start = bytes.len() - 8;
        let end_start = footer_start - 8;
        let block_len = 8 + crate::save_state::ZEFF_EXTENSION_BLOCK_LEN_FOR_TEST as usize;
        end_start - block_len..end_start
    }

    #[test]
    fn native_v13_restores_authoritative_frame_count_and_inner_lcd() {
        let mut source = emulator(false);
        source.frame_count = 0x0102_0304_0506_0708;
        source
            .bus
            .restore_ppu_lcd_framebuffer(patterned_framebuffer(0x31));
        let state = source.encode_state_bytes().unwrap();
        let expected_framebuffer = source.framebuffer().to_vec();

        let mut restored = emulator(false);
        restored.frame_count = 9;
        restored
            .bus
            .restore_ppu_lcd_framebuffer(patterned_framebuffer(0xA7));
        restored.load_state(&state).unwrap();

        assert_eq!(restored.frame_count(), source.frame_count());
        assert_bytes_equal(
            "restored framebuffer",
            restored.framebuffer(),
            &expected_framebuffer,
        );
        assert_bytes_equal(
            "re-encoded state",
            &restored.encode_state_bytes().unwrap(),
            &state,
        );

        source.step_frame();
        restored.step_frame();
        assert_eq!(restored.frame_count(), source.frame_count());
        assert_bytes_equal(
            "next framebuffer",
            restored.framebuffer(),
            source.framebuffer(),
        );
    }

    #[test]
    fn native_v13_preserves_sgb_presented_composite_and_inner_lcd_continuation() {
        let mut source = emulator(true);
        source.set_sgb_border_enabled(true);
        source.step_frame();
        source
            .bus
            .restore_ppu_lcd_framebuffer(patterned_framebuffer(0x52));
        let expected_composite = source.framebuffer().to_vec();
        let expected_inner = source.bus.ppu_lcd_framebuffer().to_vec();
        let state = source.encode_state_bytes().unwrap();

        let mut restored = emulator(true);
        restored.load_state(&state).unwrap();

        assert_bytes_equal("SGB composite", restored.framebuffer(), &expected_composite);
        assert_bytes_equal(
            "SGB inner LCD",
            restored.bus.ppu_lcd_framebuffer(),
            &expected_inner,
        );
        source.step_frame();
        restored.step_frame();
        assert_bytes_equal(
            "SGB next framebuffer",
            restored.framebuffer(),
            source.framebuffer(),
        );
    }

    #[test]
    fn native_v13_keeps_stopped_dmg_public_blank_separate_from_inner_lcd() {
        let mut source = emulator(false);
        source
            .bus
            .restore_ppu_lcd_framebuffer(patterned_framebuffer(0x73));
        let expected_inner = source.bus.ppu_lcd_framebuffer().to_vec();
        source.cpu.running = CpuState::Stopped;
        let state = source.encode_state_bytes().unwrap();

        let mut restored = emulator(false);
        restored.load_state(&state).unwrap();

        assert!(restored.framebuffer().iter().all(|&channel| channel == 255));
        assert_bytes_equal(
            "stopped DMG inner LCD",
            restored.bus.ppu_lcd_framebuffer(),
            &expected_inner,
        );
        source.set_input(1, 0);
        restored.set_input(1, 0);
        assert_bytes_equal(
            "woken DMG framebuffer",
            restored.framebuffer(),
            source.framebuffer(),
        );
    }

    #[test]
    fn replay_probe_uses_exact_v13_count_and_legacy_v12_derived_count() {
        let mut source = emulator(false);
        source.frame_count = 77;
        let current = source.encode_state_bytes().unwrap();
        let (current_frame, current_tick) = source
            .probe_native_state_for_replay(&current, None)
            .unwrap();
        assert_eq!(current_frame, 77);

        let mut legacy = current;
        crate::save_state::project_replay_state_bytes(&mut legacy).unwrap();
        let (legacy_frame, legacy_tick) =
            source.probe_native_state_for_replay(&legacy, None).unwrap();
        assert_eq!(
            legacy_frame,
            source.cpu.cycles / Emulator::cycles_per_frame(source.hardware_mode)
        );
        assert_eq!(legacy_tick, current_tick);
    }

    #[test]
    fn malformed_native_v13_is_atomic_and_never_falls_back_to_embedded_bess() {
        let source = emulator(false);
        let state = source.encode_state_bytes().unwrap();
        let extension = zeff_extension_range(&state);
        let mut missing = state.clone();
        missing.drain(extension.clone());
        let missing_before_projection = missing.clone();
        assert!(crate::save_state::project_replay_state_bytes(&mut missing).is_err());
        assert_bytes_equal("failed projection", &missing, &missing_before_projection);

        let mut target = emulator(false);
        target.step_frame();
        let before = target.encode_state_bytes().unwrap();
        let before_framebuffer = target.framebuffer().to_vec();
        let error = target.load_state(&missing).unwrap_err().to_string();
        assert!(error.contains("missing required Zeff-private `ZBEX`"));
        assert_bytes_equal(
            "atomic state",
            &target.encode_state_bytes().unwrap(),
            &before,
        );
        assert_bytes_equal(
            "atomic framebuffer",
            target.framebuffer(),
            &before_framebuffer,
        );

        let mut duplicate = state.clone();
        let block = duplicate[extension.clone()].to_vec();
        duplicate.splice(extension.end..extension.end, block);
        assert!(target.load_state(&duplicate).is_err());

        let mut wrong_length = state.clone();
        wrong_length[extension.start + 4..extension.start + 8].copy_from_slice(
            &(crate::save_state::ZEFF_EXTENSION_BLOCK_LEN_FOR_TEST - 1).to_le_bytes(),
        );
        assert!(target.load_state(&wrong_length).is_err());

        let mut oversized = state.clone();
        oversized[extension.start + 4..extension.start + 8]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(target.load_state(&oversized).is_err());

        let mut misplaced = state.clone();
        misplaced.splice(extension.end..extension.end, *b"TEST\0\0\0\0");
        assert!(target.load_state(&misplaced).is_err());

        let mut trailing_after_end = state.clone();
        let footer_start = trailing_after_end.len() - 8;
        trailing_after_end.splice(footer_start..footer_start, [0; 8 + 0x39]);
        let error = target
            .load_state(&trailing_after_end)
            .unwrap_err()
            .to_string();
        assert!(error.contains("BESS data follows END block"));

        let mut invalid_first_offset = state.clone();
        let footer_start = invalid_first_offset.len() - 8;
        invalid_first_offset[footer_start..footer_start + 4]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(target.load_state(&invalid_first_offset).is_err());

        let mut overlapping_first_offset = state.clone();
        let footer_start = overlapping_first_offset.len() - 8;
        overlapping_first_offset[footer_start..footer_start + 4]
            .copy_from_slice(&12u32.to_le_bytes());
        let error = target
            .load_state(&overlapping_first_offset)
            .unwrap_err()
            .to_string();
        assert!(error.contains("precedes completed native payload"));

        let mut truncated = state;
        truncated.remove(extension.end - 1);
        assert!(target.load_state(&truncated).is_err());
        assert_bytes_equal(
            "final atomic state",
            &target.encode_state_bytes().unwrap(),
            &before,
        );
        assert_bytes_equal(
            "final atomic framebuffer",
            target.framebuffer(),
            &before_framebuffer,
        );
    }

    #[test]
    fn wrong_rom_native_v13_failure_preserves_the_next_frame_trajectory() {
        let state = emulator(false).encode_state_bytes().unwrap();
        let mut other_rom = test_rom(false);
        other_rom[0x200] = 1;
        let mut target =
            Emulator::from_rom_data(&other_rom, HardwareModePreference::ForceDmg).unwrap();
        let mut control =
            Emulator::from_rom_data(&other_rom, HardwareModePreference::ForceDmg).unwrap();
        target.step_frame();
        control.step_frame();
        let before = target.encode_state_bytes().unwrap();
        let before_framebuffer = target.framebuffer().to_vec();

        let error = target.load_state(&state).unwrap_err().to_string();

        assert!(error.contains("ROM hash"));
        assert_bytes_equal(
            "wrong-ROM state",
            &target.encode_state_bytes().unwrap(),
            &before,
        );
        assert_bytes_equal(
            "wrong-ROM framebuffer",
            target.framebuffer(),
            &before_framebuffer,
        );
        target.step_frame();
        control.step_frame();
        assert_bytes_equal(
            "wrong-ROM next state",
            &target.encode_state_bytes().unwrap(),
            &control.encode_state_bytes().unwrap(),
        );
        assert_bytes_equal(
            "wrong-ROM next framebuffer",
            target.framebuffer(),
            control.framebuffer(),
        );
    }

    #[test]
    fn non_native_bess_import_ignores_zeff_private_extension() {
        let mut source = emulator(false);
        source
            .bus
            .restore_ppu_lcd_framebuffer(patterned_framebuffer(0x6B));
        let mut bess = source.encode_state_bytes().unwrap();
        bess[0] ^= 0xFF;

        let mut restored = emulator(false);
        restored.load_state(&bess).unwrap();
        assert_eq!(restored.cpu.pc, source.cpu.pc);
        assert!(restored.framebuffer().iter().all(|&pixel| pixel == 0));
        assert!(restored.framebuffer() != source.framebuffer());
        assert_eq!(
            restored.frame_count(),
            source.cpu.cycles / Emulator::cycles_per_frame(source.hardware_mode)
        );
    }

    #[test]
    fn non_native_bess_import_tolerates_sameboy_sgb_padding_after_end() {
        let source = emulator(true);
        let mut bess = source.encode_state_bytes().unwrap();
        bess[0] ^= 0xFF;

        let footer_start = bess.len() - 8;
        bess.splice(footer_start..footer_start, [0; 8 + 0x39]);

        let mut restored = emulator(true);
        restored.load_state(&bess).unwrap();
        assert_eq!(restored.cpu.pc, source.cpu.pc);
    }
}
