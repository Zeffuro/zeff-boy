use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;

const MAGIC: &[u8; 8] = b"ZBSEGA8\0";
const VERSION: u32 = 1;
const MAX_FRAMEBUFFER_SIZE: usize = 256 * 192 * 4;

pub fn encode_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut w = StateWriter::with_capacity(0x20_000);
    w.write_bytes(MAGIC);
    w.write_u32(VERSION);
    w.write_bytes(&emu.rom_hash);
    w.write_u64(emu.frame_count);
    w.write_u32(emu.sample_rate);
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
    if version != VERSION {
        bail!("unsupported Sega 8-bit save-state version {version}");
    }

    let mut rom_hash = [0; 32];
    r.read_exact(&mut rom_hash)?;
    if rom_hash != emu.rom_hash {
        bail!("Sega 8-bit save-state belongs to a different ROM");
    }

    emu.frame_count = r.read_u64()?;
    emu.sample_rate = r.read_u32()?.max(1);
    read_fixed_vec(
        &mut r,
        &mut emu.framebuffer,
        MAX_FRAMEBUFFER_SIZE,
        "framebuffer",
    )?;
    emu.cpu.read_state(&mut r)?;
    emu.bus.read_state(&mut r)?;
    emu.sample_rate = emu.bus().apu().sample_rate();
    emu.debug.clear_hits();
    emu.opcode_log.clear();

    if !r.is_exhausted() {
        bail!("Sega 8-bit save-state has unexpected trailing data");
    }
    Ok(())
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
        MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE, SLOT2_START,
    };

    fn test_rom() -> Vec<u8> {
        vec![0x3E, 0x5A, 0x32, 0x00, 0xC0, 0x76]
    }

    #[test]
    fn roundtrips_cpu_ram_frame_and_audio_state() {
        let rom = test_rom();
        let mut saved = Emulator::new_with_hint(&rom, 44_100, SystemHint::MasterSystem).unwrap();
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
}
