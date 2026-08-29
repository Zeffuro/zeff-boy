use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;

const MAGIC: &[u8; 8] = b"ZBSEGA8\0";
const VERSION: u32 = 12;
const MIN_SUPPORTED_VERSION: u32 = 1;
const VERSION_WITH_VIDEO_STANDARD: u32 = 3;
const VERSION_WITH_CONSOLE_REGION: u32 = 4;
const MAX_FRAMEBUFFER_SIZE: usize = 256 * 192 * 4;

pub fn encode_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut w = StateWriter::with_capacity(0x20_000);
    w.write_bytes(MAGIC);
    w.write_u32(VERSION);
    w.write_bytes(&emu.rom_hash);
    w.write_u64(emu.frame_count);
    w.write_u32(crate::emulator::DEFAULT_SAMPLE_RATE);
    w.write_u8(video_standard_to_byte(emu.video_standard));
    w.write_u8(console_region_to_byte(emu.console_region));
    w.write_vec(&emu.framebuffer);
    emu.cpu.write_state(&mut w);
    emu.bus.write_state(&mut w);
    w.write_bool(emu.bus.boot_rom_enabled());
    Ok(w.into_bytes())
}

pub fn decode_state(emu: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut r = StateReader::new(data);
    let mut magic = [0; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("not a valid Sega 8-bit save-state");
    }
    let version = r.read_u32()?;
    if !(MIN_SUPPORTED_VERSION..=VERSION).contains(&version) {
        bail!("unsupported Sega 8-bit save-state version {version}");
    }

    let mut rom_hash = [0; 32];
    r.read_exact(&mut rom_hash)?;
    if rom_hash != emu.rom_hash {
        bail!("Sega 8-bit save-state belongs to a different ROM");
    }

    emu.frame_count = r.read_u64()?;
    let _saved_sample_rate = r.read_u32()?;
    let video_standard = if version >= VERSION_WITH_VIDEO_STANDARD {
        byte_to_video_standard(r.read_u8()?)?
    } else {
        Sega8VideoStandard::default()
    };
    emu.set_video_standard(video_standard);
    let console_region = if version >= VERSION_WITH_CONSOLE_REGION {
        byte_to_console_region(r.read_u8()?)?
    } else {
        Sega8Region::default()
    };
    emu.set_console_region(console_region);
    read_fixed_vec(
        &mut r,
        &mut emu.framebuffer,
        MAX_FRAMEBUFFER_SIZE,
        "framebuffer",
    )?;
    emu.cpu.read_state(&mut r)?;
    emu.bus.read_state(&mut r, version)?;
    let boot_rom_enabled = version >= 12 && r.read_bool()?;
    if boot_rom_enabled && !emu.bus.has_boot_rom() {
        bail!("Sega 8-bit save state is still executing the boot ROM, but none is loaded");
    }
    if boot_rom_enabled || emu.bus.has_boot_rom() {
        emu.bus.set_boot_rom_enabled(boot_rom_enabled);
    }
    emu.sample_rate = emu.bus().apu().sample_rate();
    emu.debug.clear_hits();
    emu.opcode_log.clear();
    emu.instruction_trace.clear();

    if !r.is_exhausted() {
        bail!("Sega 8-bit save-state has unexpected trailing data");
    }
    Ok(())
}

fn video_standard_to_byte(video_standard: Sega8VideoStandard) -> u8 {
    match video_standard {
        Sega8VideoStandard::Ntsc => 0,
        Sega8VideoStandard::Pal => 1,
    }
}

fn byte_to_video_standard(value: u8) -> anyhow::Result<Sega8VideoStandard> {
    match value {
        0 => Ok(Sega8VideoStandard::Ntsc),
        1 => Ok(Sega8VideoStandard::Pal),
        _ => anyhow::bail!("invalid Sega 8-bit video standard tag in save-state: {value}"),
    }
}

fn console_region_to_byte(console_region: Sega8Region) -> u8 {
    match console_region {
        Sega8Region::Export => 0,
        Sega8Region::Japanese => 1,
        Sega8Region::JapanesePowerBaseConverter => 2,
    }
}

fn byte_to_console_region(value: u8) -> anyhow::Result<Sega8Region> {
    match value {
        0 => Ok(Sega8Region::Export),
        1 => Ok(Sega8Region::Japanese),
        2 => Ok(Sega8Region::JapanesePowerBaseConverter),
        _ => anyhow::bail!("invalid Sega 8-bit console region tag in save-state: {value}"),
    }
}

fn read_fixed_vec(
    r: &mut StateReader<'_>,
    out: &mut Vec<u8>,
    max_len: usize,
    label: &str,
) -> anyhow::Result<()> {
    let bytes = r.read_vec(max_len)?;
    if bytes.len() != out.len() {
        bail!(
            "Sega 8-bit save-state {label} size mismatch: expected {}, got {}",
            out.len(),
            bytes.len()
        );
    }
    *out = bytes;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::Sega8LoadConfig;
    use crate::hardware::cartridge::{Sega8MapperKind, SystemHint};
    use crate::hardware::constants::{
        IO_OPEN_BUS_VALUE, IO_PORT_CONTROL, IO_PORT_CONTROLLER_2, IO_PORT_GG_START,
        IO_PORT_MEMORY_CONTROL, IO_PORT_VDP_CONTROL, IO_PORT_VDP_DATA, MAPPER_FRAME_CONTROL,
        MAPPER_FRAME_CONTROL_CART_RAM_ENABLE, ROM_BANK_SIZE, ROM_PAGE_8K_SIZE, SLOT2_START,
        SMS_CARTRIDGE_RAM_SIZE, SMS_CRAM_SIZE, SMS_VDP_REGISTER_COUNT, SMS_VRAM_SIZE,
        SMS_WORK_RAM_SIZE,
    };

    const VDP_SCANLINE_DISPLAY_HISTORY_BYTES: usize = 240;

    fn test_rom() -> Vec<u8> {
        vec![0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76]
    }

    fn banked_rom(bank_count: usize) -> Vec<u8> {
        let mut rom = vec![0; bank_count * ROM_BANK_SIZE];
        for bank in 0..bank_count {
            rom[bank * ROM_BANK_SIZE..(bank + 1) * ROM_BANK_SIZE].fill(bank as u8);
        }
        rom
    }

    fn paged_rom_8k(page_count: usize) -> Vec<u8> {
        let mut rom = vec![0; page_count * ROM_PAGE_8K_SIZE];
        for page in 0..page_count {
            rom[page * ROM_PAGE_8K_SIZE..(page + 1) * ROM_PAGE_8K_SIZE].fill(page as u8);
        }
        rom
    }

    fn set_state_version(bytes: &mut Vec<u8>, version: u32) {
        if version < 12 {
            bytes
                .pop()
                .expect("current state should include boot-ROM overlay state");
        }
        bytes[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&version.to_le_bytes());
    }

    fn video_standard_offset() -> usize {
        MAGIC.len() + 4 + 32 + 8 + 4
    }

    fn console_region_offset() -> usize {
        video_standard_offset() + 1
    }

    fn strip_game_gear_serial_state(bytes: &mut Vec<u8>) {
        for _ in 0..6 {
            bytes
                .pop()
                .expect("state should include Game Gear serial bytes");
        }
    }

    fn strip_game_gear_serial_timing_state(bytes: &mut Vec<u8>) {
        for _ in 0..4 {
            bytes
                .pop()
                .expect("v9 state should include Game Gear serial timing bytes");
        }
    }

    fn strip_memory_control_state(bytes: &mut Vec<u8>) {
        bytes
            .pop()
            .expect("v8 state should end with memory control byte");
    }

    fn strip_vdp_cram_latch_state(bytes: &mut Vec<u8>) {
        bytes
            .pop()
            .expect("v10 state should end with VDP CRAM latch byte");
    }

    fn strip_vdp_scanline_display_state(bytes: &mut Vec<u8>) {
        let start = vdp_scanline_display_history_offset(bytes);
        bytes.drain(start..start + VDP_SCANLINE_DISPLAY_HISTORY_BYTES);
    }

    fn vdp_scanline_display_history_offset(bytes: &[u8]) -> usize {
        let mut r = StateReader::new(bytes);
        let mut magic = [0; 8];
        r.read_exact(&mut magic).unwrap();
        r.read_u32().unwrap();
        let mut rom_hash = [0; 32];
        r.read_exact(&mut rom_hash).unwrap();
        r.read_u64().unwrap();
        r.read_u32().unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_vec(MAX_FRAMEBUFFER_SIZE).unwrap();
        skip_cpu_state(&mut r);
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        for _ in 0..3 {
            r.read_u8().unwrap();
        }
        r.read_vec(SMS_WORK_RAM_SIZE).unwrap();
        r.read_vec(SMS_CARTRIDGE_RAM_SIZE).unwrap();
        r.read_vec(SMS_VRAM_SIZE).unwrap();
        r.read_vec(SMS_CRAM_SIZE).unwrap();
        r.read_vec(SMS_VDP_REGISTER_COUNT).unwrap();
        r.read_u16().unwrap();
        r.read_u8().unwrap();
        r.read_bool().unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_u16().unwrap();
        r.read_u32().unwrap();
        r.position()
    }

    fn skip_cpu_state(r: &mut StateReader<'_>) {
        let mut registers = [0; 18];
        r.read_exact(&mut registers).unwrap();
        let mut shadow_registers = [0; 8];
        r.read_exact(&mut shadow_registers).unwrap();
        r.read_u8().unwrap();
        r.read_u8().unwrap();
        r.read_bool().unwrap();
        r.read_bool().unwrap();
        r.read_u8().unwrap();
        r.read_u64().unwrap();
        r.read_u16().unwrap();
        r.read_u8().unwrap();
        match r.read_u8().unwrap() {
            0 => {}
            1 => {
                r.read_u16().unwrap();
                r.read_u8().unwrap();
            }
            2 => {
                r.read_u16().unwrap();
                r.read_u8().unwrap();
                r.read_u8().unwrap();
            }
            tag => panic!("unexpected CPU trap tag in test save state: {tag}"),
        }
    }

    #[test]
    fn roundtrips_cpu_ram_frame_and_audio_state() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint_and_video_standard(
            &rom,
            44_100,
            SystemHint::MasterSystem,
            Sega8VideoStandard::Pal,
        )
        .unwrap();
        saved.set_console_region(Sega8Region::JapanesePowerBaseConverter);
        saved.bus_mut().io_write(IO_PORT_CONTROL, 0x55);
        saved.step_instruction();
        saved.step_instruction();
        saved.step_frame();
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.bus().cpu_read(0xC000), 0x5A);
        assert_eq!(restored.frame_count(), saved.frame_count());
        assert_eq!(restored.framebuffer(), saved.framebuffer());
        assert_eq!(restored.sample_rate(), 48_000);
        assert_eq!(restored.video_standard(), Sega8VideoStandard::Pal);
        assert_eq!(
            restored.console_region(),
            Sega8Region::JapanesePowerBaseConverter
        );
        assert_eq!(restored.bus().input().io_control(), 0x55);
        assert_eq!(
            restored.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0,
            0xC0
        );
    }

    #[test]
    fn decode_preserves_runtime_audio_config() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 44_100, SystemHint::MasterSystem).unwrap();
        saved.set_apu_sample_generation_enabled(true);
        saved.set_apu_channel_mutes([false, false, false, false]);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 96_000, SystemHint::MasterSystem).unwrap();
        restored.set_apu_sample_generation_enabled(false);
        restored.set_apu_channel_mutes([true, false, true, false]);

        decode_state(&mut restored, &bytes).unwrap();

        let apu = restored.bus().apu().debug_snapshot();
        assert_eq!(restored.sample_rate(), 96_000);
        assert_eq!(apu.sample_rate, 96_000);
        assert!(!apu.sample_generation_enabled);
        assert_eq!(apu.channel_mutes, [true, false, true, false]);
    }

    #[test]
    fn pal_state_load_restores_pal_sample_cadence_with_runtime_audio_rate() {
        let rom = test_rom();
        let saved = Emulator::new_with_hint_and_video_standard(
            &rom,
            48_000,
            SystemHint::MasterSystem,
            Sega8VideoStandard::Pal,
        )
        .unwrap();
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 44_100, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        restored.step_frame();

        let mut samples = Vec::new();
        restored.drain_audio_samples_into(&mut samples);
        assert_eq!(restored.video_standard(), Sega8VideoStandard::Pal);
        assert_eq!(restored.sample_rate(), 44_100);
        assert_eq!(samples.len(), 1_764);
    }

    #[test]
    fn roundtrips_sms_memory_control_state() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        saved.bus_mut().cpu_write(0xC000, 0x5A);
        saved.bus_mut().io_write(IO_PORT_MEMORY_CONTROL, 0x10);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.bus().memory_control(), 0x10);
        assert_eq!(restored.bus().cpu_read(0xC000), IO_OPEN_BUS_VALUE);
        restored.bus_mut().io_write(IO_PORT_MEMORY_CONTROL, 0x00);
        assert_eq!(restored.bus().cpu_read(0xC000), 0x5A);
    }

    #[test]
    fn rejects_cross_rom_state() {
        let saved_rom = test_rom();
        let mut other_rom = test_rom();
        other_rom[0] = 0x00;
        let saved = Emulator::new_with_hint(&saved_rom, 48_000, SystemHint::MasterSystem).unwrap();
        let bytes = encode_state(&saved).unwrap();

        let mut restored =
            Emulator::new_with_hint(&other_rom, 48_000, SystemHint::MasterSystem).unwrap();
        let err = decode_state(&mut restored, &bytes).unwrap_err();

        assert!(
            err.to_string().contains("different ROM"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn roundtrips_mapper_cartridge_ram() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        saved
            .bus_mut()
            .cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
        saved.bus_mut().cpu_write(SLOT2_START, 0xA5);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert!(restored.bus().mapper().slot2_cartridge_ram_enabled());
        assert_eq!(restored.bus().cpu_read(SLOT2_START), 0xA5);
    }

    #[test]
    fn roundtrips_korean_mapper_state() {
        let rom = banked_rom(4);
        let config = Sega8LoadConfig::new(48_000)
            .with_system_hint(SystemHint::MasterSystem)
            .with_mapper_kind(Some(Sega8MapperKind::Korean));
        let mut saved = Emulator::new_with_config(&rom, config).unwrap();
        saved.bus_mut().cpu_write(0xA000, 3);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_config(&rom, config).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.bus().mapper().kind(), Sega8MapperKind::Korean);
        assert_eq!(restored.bus().mapper().slot_banks(), [0, 1, 3]);
        assert_eq!(restored.bus().cpu_read(SLOT2_START), 3);
    }

    #[test]
    fn roundtrips_msx_mapper_state() {
        let rom = paged_rom_8k(8);
        let config = Sega8LoadConfig::new(48_000)
            .with_system_hint(SystemHint::MasterSystem)
            .with_mapper_kind(Some(Sega8MapperKind::Msx));
        let mut saved = Emulator::new_with_config(&rom, config).unwrap();
        saved.bus_mut().cpu_write(0x0000, 7);
        saved.bus_mut().cpu_write(0x0001, 6);
        saved.bus_mut().cpu_write(0x0002, 5);
        saved.bus_mut().cpu_write(0x0003, 4);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_config(&rom, config).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.bus().mapper().kind(), Sega8MapperKind::Msx);
        assert_eq!(restored.bus().cpu_read(0x4000), 5);
        assert_eq!(restored.bus().cpu_read(0x6000), 4);
        assert_eq!(restored.bus().cpu_read(0x8000), 7);
        assert_eq!(restored.bus().cpu_read(0xA000), 6);
    }

    #[test]
    fn roundtrips_janggun_mapper_state() {
        let rom = paged_rom_8k(16);
        let config = Sega8LoadConfig::new(48_000)
            .with_system_hint(SystemHint::MasterSystem)
            .with_mapper_kind(Some(Sega8MapperKind::Janggun));
        let mut saved = Emulator::new_with_config(&rom, config).unwrap();
        saved.bus_mut().cpu_write(0x4000, 0x46);
        saved.bus_mut().cpu_write(0xFFFF, 0x04);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_config(&rom, config).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.bus().mapper().kind(), Sega8MapperKind::Janggun);
        assert_eq!(restored.bus().cpu_read(0x4000), 6u8.reverse_bits());
        assert_eq!(restored.bus().cpu_read(0x8000), 4);
        assert_eq!(restored.bus().cpu_read(0xA000), 5);
    }

    #[test]
    fn roundtrips_game_gear_start_input() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        saved.set_input(0x08, 0x00);
        saved
            .bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL, 0x30);
        saved
            .bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_GG_SERIAL_TX, 0xA5);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert!(restored.bus().input().game_gear_start_pressed());
        assert_eq!(restored.bus_mut().io_read(IO_PORT_GG_START), 0x7F);
        assert_eq!(
            restored
                .bus_mut()
                .io_read(crate::hardware::constants::IO_PORT_GG_SERIAL_TX),
            0xA5
        );
        assert_eq!(
            restored
                .bus_mut()
                .io_read(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL)
                & 0x34,
            0x34
        );
    }

    #[test]
    fn decodes_v2_state_with_game_gear_start_but_without_video_standard() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint_and_video_standard(
            &rom,
            48_000,
            SystemHint::GameGear,
            Sega8VideoStandard::Pal,
        )
        .unwrap();
        saved.set_input(0x08, 0x00);
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 2);
        bytes.remove(console_region_offset());
        bytes.remove(video_standard_offset());
        strip_vdp_cram_latch_state(&mut bytes);
        strip_memory_control_state(&mut bytes);
        strip_game_gear_serial_timing_state(&mut bytes);
        strip_game_gear_serial_state(&mut bytes);
        bytes
            .pop()
            .expect("current state should include IO control before Game Gear serial bytes");

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.video_standard(), Sega8VideoStandard::Ntsc);
        assert_eq!(restored.console_region(), Sega8Region::Export);
        assert!(restored.bus().input().game_gear_start_pressed());
        assert_eq!(restored.bus_mut().io_read(IO_PORT_GG_START), 0x7F);
    }

    #[test]
    fn decodes_v3_state_with_video_standard_but_without_console_region_or_io_control() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint_and_video_standard(
            &rom,
            48_000,
            SystemHint::MasterSystem,
            Sega8VideoStandard::Pal,
        )
        .unwrap();
        saved.set_console_region(Sega8Region::Japanese);
        saved.bus_mut().io_write(IO_PORT_CONTROL, 0x55);
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 3);
        bytes.remove(console_region_offset());
        strip_vdp_cram_latch_state(&mut bytes);
        strip_memory_control_state(&mut bytes);
        strip_game_gear_serial_timing_state(&mut bytes);
        strip_game_gear_serial_state(&mut bytes);
        bytes
            .pop()
            .expect("current state should include IO control before Game Gear serial bytes");

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.video_standard(), Sega8VideoStandard::Pal);
        assert_eq!(restored.console_region(), Sega8Region::Export);
        assert_eq!(restored.bus().input().io_control(), 0xFF);
        assert_eq!(restored.bus().memory_control(), 0x00);
        restored.bus_mut().io_write(IO_PORT_CONTROL, 0x55);
        assert_eq!(restored.bus_mut().io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0);
    }

    #[test]
    fn decodes_v1_state_without_game_gear_start_input() {
        let rom = test_rom();
        let saved = Emulator::new_with_hint_and_video_standard(
            &rom,
            48_000,
            SystemHint::GameGear,
            Sega8VideoStandard::Pal,
        )
        .unwrap();
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 1);
        bytes.remove(console_region_offset());
        bytes.remove(video_standard_offset());
        strip_vdp_cram_latch_state(&mut bytes);
        strip_memory_control_state(&mut bytes);
        strip_game_gear_serial_timing_state(&mut bytes);
        strip_game_gear_serial_state(&mut bytes);
        bytes
            .pop()
            .expect("current state should include IO control before Game Gear serial bytes");
        bytes
            .pop()
            .expect("v2+ state should end with GG start byte");

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert!(!restored.bus().input().game_gear_start_pressed());
        assert_eq!(restored.bus_mut().io_read(IO_PORT_GG_START), 0xFF);
        assert_eq!(restored.video_standard(), Sega8VideoStandard::Ntsc);
        assert_eq!(restored.console_region(), Sega8Region::Export);
    }

    #[test]
    fn decodes_v5_state_without_game_gear_serial_state() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        saved
            .bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL, 0x30);
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 5);
        strip_vdp_cram_latch_state(&mut bytes);
        strip_memory_control_state(&mut bytes);
        strip_game_gear_serial_timing_state(&mut bytes);
        strip_game_gear_serial_state(&mut bytes);

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(
            restored
                .bus_mut()
                .io_read(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL),
            0
        );
    }

    #[test]
    fn decodes_v6_state_without_game_gear_serial_flags() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        saved
            .bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL, 0x30);
        saved
            .bus_mut()
            .io_write(crate::hardware::constants::IO_PORT_GG_SERIAL_TX, 0xA5);
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 6);
        strip_vdp_cram_latch_state(&mut bytes);
        strip_memory_control_state(&mut bytes);
        strip_game_gear_serial_timing_state(&mut bytes);
        bytes.pop().expect("v7 state should include serial flags");

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(
            restored
                .bus_mut()
                .io_read(crate::hardware::constants::IO_PORT_GG_SERIAL_TX),
            0xA5
        );
        assert_eq!(
            restored
                .bus_mut()
                .io_read(crate::hardware::constants::IO_PORT_GG_SERIAL_CONTROL)
                & 0x03,
            0
        );
    }

    #[test]
    fn decodes_v9_state_without_vdp_cram_latch() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        saved.bus_mut().io_write(IO_PORT_VDP_CONTROL, 0x00);
        saved.bus_mut().io_write(IO_PORT_VDP_CONTROL, 0xC0);
        saved.bus_mut().io_write(IO_PORT_VDP_DATA, 0x7B);
        let mut bytes = encode_state(&saved).unwrap();
        strip_vdp_scanline_display_state(&mut bytes);
        set_state_version(&mut bytes, 9);
        strip_vdp_cram_latch_state(&mut bytes);

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::GameGear).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        restored.bus_mut().io_write(IO_PORT_VDP_DATA, 0x05);

        assert_eq!(restored.bus().vdp().cram()[0], 0x00);
        assert_eq!(restored.bus().vdp().cram()[1], 0x05);
    }

    #[test]
    fn public_load_rejects_trailing_data_without_mutation() {
        let mut emu = Emulator::new(&test_rom(), 48_000).unwrap();
        emu.set_input(0x03, 0x05);
        emu.step_frame();
        let before = emu.encode_state().unwrap();
        let framebuffer = emu.framebuffer().to_vec();
        let mut expected = emu.clone();
        let mut invalid = before.clone();
        invalid.push(0xA5);

        assert!(emu.load_state(&invalid).is_err());
        assert_eq!(emu.encode_state().unwrap(), before);
        assert_eq!(emu.framebuffer(), framebuffer);
        assert_eq!(emu.drain_audio_samples(), expected.drain_audio_samples());
    }
}
