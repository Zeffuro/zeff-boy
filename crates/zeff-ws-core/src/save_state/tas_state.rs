use anyhow::{bail, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_ram::SaveRamKind;

use super::{SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, decode_state, encode_state};
use crate::emulator::Emulator;
use crate::hardware::cartridge::{MinimumSystem, RomFooter, RomOrientation, SaveKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WonderSwanTasStartup {
    InternalPostBoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasKeypadState {
    pub select: u8,
    pub x_buttons: u8,
    pub y_buttons: u8,
    pub ab_start: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasRtcState {
    pub command: u8,
    pub payload: [u8; 7],
    pub payload_index: u8,
    pub payload_len: u8,
    pub ready_delay_reads: u8,
    pub invalid_command: bool,
    pub subsecond_cycles: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasUartState {
    pub rx_data: u8,
    pub rx_ready: bool,
    pub overrun: bool,
    pub tx_data: u8,
    pub tx_pending: bool,
    pub tx_cycles_remaining: u32,
    pub completed_tx_events: Vec<crate::hardware::bus::WonderSwanTxEvent>,
    pub tx_generation: u64,
    pub control: u8,
}

impl CurrentNativeWonderSwanTasUartState {
    pub fn is_disconnected(&self) -> bool {
        self.control == 0
            && !self.rx_ready
            && !self.overrun
            && !self.tx_pending
            && self.tx_cycles_remaining == 0
            && self.completed_tx_events.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasDeferredBankState {
    pub value: u8,
    pub remaining_instruction_retires: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeWonderSwanTasStateInspection {
    pub projection: CurrentNativeWonderSwanTasStateProjection,
    pub rom_sha256: [u8; 32],
    pub rom_len: usize,
    pub rom_footer: RomFooter,
    pub minimum_system: MinimumSystem,
    pub color_hardware: bool,
    pub color_mode_active: bool,
    pub system_control: u8,
    pub video_mode: u8,
    pub orientation: RomOrientation,
    pub save_kind: SaveKind,
    pub save_ram_kind: SaveRamKind,
    pub cartridge_save_len: usize,
    pub cartridge_save_sha256: [u8; 32],
    pub internal_eeprom_len: usize,
    pub internal_eeprom_sha256: [u8; 32],
    pub rtc_present: bool,
    pub rtc: CurrentNativeWonderSwanTasRtcState,
    pub keypad: CurrentNativeWonderSwanTasKeypadState,
    pub uart: CurrentNativeWonderSwanTasUartState,
    pub deferred_linear_bank: Option<CurrentNativeWonderSwanTasDeferredBankState>,
    pub startup: WonderSwanTasStartup,
}

pub fn inspect_current_native_wonder_swan_tas_state(
    emulator: &Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeWonderSwanTasStateInspection> {
    ensure!(
        data.len() >= 9 && data[..8] == SAVE_STATE_MAGIC,
        "TAS requires a native WonderSwan save state"
    );
    ensure!(
        data[8] == SAVE_STATE_FORMAT_VERSION,
        "TAS requires WonderSwan save-state format {SAVE_STATE_FORMAT_VERSION}"
    );

    let mut candidate = emulator.clone();
    decode_state(&mut candidate, data)?;
    if encode_state(&candidate)? != data {
        bail!("WonderSwan TAS state is not canonical current-native data");
    }

    let footer = candidate.bus.cartridge.footer().clone();
    let keypad = candidate.bus.keypad.save_state();
    let rtc = candidate.bus.rtc_save_state();
    ensure!(
        crate::hardware::bus::valid_rtc_datetime(rtc.payload),
        "WonderSwan TAS state has an invalid RTC date/time"
    );
    let uart = candidate.bus.uart_save_state();
    let video_mode = candidate.bus.io[0x60];
    let system_control = candidate.bus.io[0xA0];
    let cartridge_save = candidate.bus.cartridge.save_data();
    let internal_eeprom = &candidate.bus.internal_eeprom;

    Ok(CurrentNativeWonderSwanTasStateInspection {
        projection: CurrentNativeWonderSwanTasStateProjection {
            replay_state_bytes: data.to_vec(),
            frame_count: candidate.frame_count(),
            framebuffer: candidate.framebuffer().into(),
        },
        rom_sha256: candidate.rom_hash(),
        rom_len: candidate.cartridge_rom_bytes().len(),
        minimum_system: footer.minimum_system,
        color_hardware: candidate.bus.is_color_model(),
        color_mode_active: candidate.bus.is_color_model() && video_mode & 0x80 != 0,
        system_control,
        video_mode,
        orientation: footer.orientation(),
        save_kind: footer.save_kind,
        save_ram_kind: candidate.save_ram_kind(),
        cartridge_save_len: cartridge_save.len(),
        cartridge_save_sha256: Sha256::digest(cartridge_save).into(),
        internal_eeprom_len: internal_eeprom.len(),
        internal_eeprom_sha256: Sha256::digest(internal_eeprom).into(),
        rtc_present: footer.rtc_present,
        rtc: CurrentNativeWonderSwanTasRtcState {
            command: rtc.command,
            payload: rtc.payload,
            payload_index: rtc.payload_index,
            payload_len: rtc.payload_len,
            ready_delay_reads: rtc.ready_delay_reads,
            invalid_command: rtc.invalid_command,
            subsecond_cycles: rtc.subsecond_cycles,
        },
        keypad: CurrentNativeWonderSwanTasKeypadState {
            select: keypad.select,
            x_buttons: keypad.x_buttons,
            y_buttons: keypad.y_buttons,
            ab_start: keypad.ab_start,
        },
        uart: CurrentNativeWonderSwanTasUartState {
            rx_data: uart.rx_data,
            rx_ready: uart.rx_ready,
            overrun: uart.overrun,
            tx_data: uart.tx_data,
            tx_pending: uart.tx_pending,
            tx_cycles_remaining: uart.tx_cycles_remaining,
            completed_tx_events: uart.completed_tx_events,
            tx_generation: uart.tx_generation,
            control: candidate.bus.io[0xB3],
        },
        deferred_linear_bank: candidate.bus.deferred_linear_bank_save_values().map(
            |(value, remaining_instruction_retires)| CurrentNativeWonderSwanTasDeferredBankState {
                value,
                remaining_instruction_retires,
            },
        ),
        rom_footer: footer,
        startup: WonderSwanTasStartup::InternalPostBoot,
    })
}

pub fn validate_and_load_current_native_wonder_swan_tas_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeWonderSwanTasStateProjection> {
    let inspection = inspect_current_native_wonder_swan_tas_state(emulator, data)?;
    emulator.load_state(data)?;
    Ok(inspection.projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::compute_footer_checksum;

    fn rom(system: u8, save_kind: u8, orientation: u8, rtc_present: bool) -> Vec<u8> {
        let mut rom = vec![0x90; 0x1_0000];
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer + 1] = system;
        rom[footer + 4] = 0x01;
        rom[footer + 5] = save_kind;
        rom[footer + 6] = orientation;
        rom[footer + 7] = u8::from(rtc_present);
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn identifiers_track_current_state() {
        assert_eq!(
            super::super::TAS_STATE_FORMAT_COMPATIBILITY_ID,
            format!("zeff-ws-native-state-v{SAVE_STATE_FORMAT_VERSION}")
        );
        assert!(!super::super::TAS_DETERMINISM_ABI_ID.is_empty());
    }

    #[test]
    fn current_state_restores_input_rtc_deferred_bank_and_continuation() {
        let rom = rom(1, 0, 1, true);
        let mut source = Emulator::from_rom_data(&rom).unwrap();
        source.step_frame();
        source.set_input(0b0011_1011, 0b0101);
        source.io_write8(0xB5, 0x70);
        source.io_write8(0xCB, 0x24);
        source.io_write8(0xCA, 0x14);
        source.io_write8(0xCB, 0x08);
        source.io_write8(0xCB, 0x31);
        source.io_write8(0xB3, 0x80);
        source.io_write8(0xB1, 0xA5);
        source.bus.step_cycles(100);
        source.io_write8(0xC0, 2);
        let state = source.encode_state().unwrap();

        let mut restored = Emulator::from_rom_data(&rom).unwrap();
        let inspection = inspect_current_native_wonder_swan_tas_state(&restored, &state).unwrap();
        assert_eq!(inspection.minimum_system, MinimumSystem::WonderSwanColor);
        assert_eq!(inspection.rom_len, rom.len());
        assert!(inspection.color_hardware);
        assert_eq!(inspection.orientation, RomOrientation::Vertical);
        assert_eq!(inspection.save_kind, SaveKind::None);
        assert_eq!(inspection.save_ram_kind, SaveRamKind::None);
        assert!(inspection.rtc_present);
        assert_eq!(inspection.keypad.select, 0x70);
        assert_eq!(inspection.keypad.x_buttons, 0x05);
        assert_eq!(inspection.keypad.y_buttons, 0x03);
        assert_eq!(inspection.keypad.ab_start, 0x0E);
        assert_eq!(inspection.rtc.payload_index, 3);
        assert!(inspection.uart.tx_pending);
        assert!(!inspection.uart.is_disconnected());
        assert_eq!(inspection.uart.tx_data, 0xA5);
        assert_eq!(
            inspection.deferred_linear_bank,
            Some(CurrentNativeWonderSwanTasDeferredBankState {
                value: 2,
                remaining_instruction_retires: 2,
            })
        );
        assert_eq!(inspection.startup, WonderSwanTasStartup::InternalPostBoot);

        let projection =
            validate_and_load_current_native_wonder_swan_tas_state(&mut restored, &state).unwrap();
        assert_eq!(projection.replay_state_bytes, state);
        assert_eq!(projection.frame_count, source.frame_count());
        assert_eq!(projection.framebuffer.as_ref(), source.framebuffer());
        assert_eq!(
            restored.encode_state().unwrap(),
            source.encode_state().unwrap()
        );

        assert_eq!(restored.io_read8(0xCA), source.io_read8(0xCA));
        assert_eq!(restored.io_read8(0xCB), source.io_read8(0xCB));
        restored.step_instruction();
        source.step_instruction();
        assert_eq!(
            restored.encode_state().unwrap(),
            source.encode_state().unwrap()
        );
    }

    #[test]
    fn current_state_restores_rtc_timebase_across_second_tick() {
        let rom = rom(1, 0, 0, true);
        let mut source = Emulator::from_rom_data(&rom).unwrap();
        source
            .bus
            .step_cycles(crate::hardware::constants::CPU_CLOCK_HZ - 17);
        let state = source.encode_state().unwrap();
        let inspection = inspect_current_native_wonder_swan_tas_state(&source, &state).unwrap();
        assert_eq!(inspection.rtc.payload, [0x00, 0x01, 0x01, 0x06, 0, 0, 0]);
        assert_eq!(
            inspection.rtc.subsecond_cycles,
            crate::hardware::constants::CPU_CLOCK_HZ - 17
        );

        let mut restored = Emulator::from_rom_data(&rom).unwrap();
        validate_and_load_current_native_wonder_swan_tas_state(&mut restored, &state).unwrap();
        source.bus.step_cycles(34);
        restored.bus.step_cycles(34);

        let rtc = restored.bus.rtc_save_state();
        assert_eq!(rtc.payload, [0x00, 0x01, 0x01, 0x06, 0, 0, 1]);
        assert_eq!(rtc.subsecond_cycles, 17);
        assert_eq!(
            restored.encode_state().unwrap(),
            source.encode_state().unwrap()
        );
    }

    #[test]
    fn current_state_rejects_legacy_malformed_and_wrong_identity_atomically() {
        let rom_bytes = rom(0, 0, 0, false);
        let source = Emulator::from_rom_data(&rom_bytes).unwrap();
        let current = source.encode_state().unwrap();
        assert!(
            inspect_current_native_wonder_swan_tas_state(&source, &current)
                .unwrap()
                .uart
                .is_disconnected()
        );
        let mut target = Emulator::from_rom_data(&rom_bytes).unwrap();
        target.set_input(1, 2);
        target.step_frame();
        let before = target.encode_state().unwrap();

        let mut legacy = current[..current.len() - super::super::V14_EXTENSION_LEN].to_vec();
        legacy[8] = 12;
        let mut bad_magic = current.clone();
        bad_magic[0] ^= 0xFF;
        let mut malformed = current.clone();
        malformed[current.len() - super::super::V14_EXTENSION_LEN] = 0xFF;
        let mut invalid_rtc_phase = current.clone();
        let rtc_phase = invalid_rtc_phase.len() - 7;
        invalid_rtc_phase[rtc_phase..rtc_phase + 4]
            .copy_from_slice(&crate::hardware::constants::CPU_CLOCK_HZ.to_le_bytes());
        let mut trailing = current.clone();
        trailing.push(0);
        let truncated = &current[..current.len() - 1];

        for invalid in [
            &legacy[..],
            &bad_magic,
            &malformed,
            &invalid_rtc_phase,
            &trailing,
            truncated,
        ] {
            assert!(
                validate_and_load_current_native_wonder_swan_tas_state(&mut target, invalid)
                    .is_err()
            );
            assert_eq!(target.encode_state().unwrap(), before);
        }

        let other_rom = rom(1, 0, 0, false);
        let wrong_system_state = Emulator::from_rom_data(&other_rom)
            .unwrap()
            .encode_state()
            .unwrap();
        assert!(
            validate_and_load_current_native_wonder_swan_tas_state(
                &mut target,
                &wrong_system_state,
            )
            .is_err()
        );
        assert_eq!(target.encode_state().unwrap(), before);
    }

    #[test]
    fn non_rtc_cartridge_does_not_advance_the_rtc_timebase() {
        let rom = rom(0, 0, 0, false);
        let mut emulator = Emulator::from_rom_data(&rom).unwrap();
        let before = emulator.bus.rtc_save_state();
        emulator
            .bus
            .step_cycles(crate::hardware::constants::CPU_CLOCK_HZ + 19);
        assert_eq!(emulator.bus.rtc_save_state(), before);
    }

    #[test]
    fn ordinary_v12_load_preserves_legacy_unsaved_runtime_domains() {
        let rom = rom(0, 0, 0, false);
        let current = Emulator::from_rom_data(&rom)
            .unwrap()
            .encode_state()
            .unwrap();
        let mut legacy = current[..current.len() - super::super::V14_EXTENSION_LEN].to_vec();
        legacy[8] = 12;
        let mut target = Emulator::from_rom_data(&rom).unwrap();
        target.set_input(0x31, 0x06);
        target.io_write8(0xB5, 0x70);
        target.io_write8(0xCB, 0x25);
        target.io_write8(0xCA, 0x14);
        target.io_write8(0xC0, 4);
        let keypad = target.bus.keypad.save_state();
        let rtc = target.bus.rtc_save_state();
        let pending = target.bus.deferred_linear_bank_save_values();

        target.load_state(&legacy).unwrap();

        assert_eq!(target.bus.keypad.save_state(), keypad);
        assert_eq!(target.bus.rtc_save_state(), rtc);
        assert_eq!(target.bus.deferred_linear_bank_save_values(), pending);
    }
}
