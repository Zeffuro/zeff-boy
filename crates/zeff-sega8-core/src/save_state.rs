use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;

const MAGIC: &[u8; 8] = b"ZBSEGA8\0";
const VERSION: u32 = 7;
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
    w.write_u32(emu.sample_rate);
    w.write_u8(video_standard_to_byte(emu.video_standard));
    w.write_u8(console_region_to_byte(emu.console_region));
    w.write_vec(&emu.framebuffer);
    emu.cpu.write_state(&mut w);
    emu.bus.write_state(&mut w);
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
    emu.sample_rate = r.read_u32()?.max(1);
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
    emu.sample_rate = emu.bus().apu().sample_rate();
    emu.debug.clear_hits();
    emu.opcode_log.clear();

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
    use crate::hardware::cartridge::SystemHint;
    use crate::hardware::constants::{
        IO_PORT_CONTROL, IO_PORT_CONTROLLER_2, IO_PORT_GG_START, MAPPER_FRAME_CONTROL,
        MAPPER_FRAME_CONTROL_CART_RAM_ENABLE, SLOT2_START,
    };

    fn test_rom() -> Vec<u8> {
        vec![0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76]
    }

    fn set_state_version(bytes: &mut [u8], version: u32) {
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
                .expect("v7 state should end with Game Gear serial bytes");
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
        assert_eq!(restored.sample_rate(), saved.sample_rate());
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
        set_state_version(&mut bytes, 2);
        bytes.remove(console_region_offset());
        bytes.remove(video_standard_offset());
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
        set_state_version(&mut bytes, 3);
        bytes.remove(console_region_offset());
        strip_game_gear_serial_state(&mut bytes);
        bytes
            .pop()
            .expect("current state should include IO control before Game Gear serial bytes");

        let mut restored = Emulator::new_with_hint(&rom, 48_000, SystemHint::MasterSystem).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.video_standard(), Sega8VideoStandard::Pal);
        assert_eq!(restored.console_region(), Sega8Region::Export);
        assert_eq!(restored.bus().input().io_control(), 0xFF);
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
        set_state_version(&mut bytes, 1);
        bytes.remove(console_region_offset());
        bytes.remove(video_standard_offset());
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
        set_state_version(&mut bytes, 5);
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
        set_state_version(&mut bytes, 6);
        bytes.pop().expect("v7 state should end with serial flags");

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
}
