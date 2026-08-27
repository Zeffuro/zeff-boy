use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;

const MAGIC: &[u8; 8] = b"ZBCOLCO\0";
const VERSION: u32 = 1;

pub fn encode_state(emulator: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut writer = StateWriter::with_capacity(0x40_000);
    writer.write_bytes(MAGIC);
    writer.write_u32(VERSION);
    writer.write_bytes(&emulator.bios_hash);
    writer.write_bytes(&emulator.cartridge_hash);
    writer.write_u64(emulator.effective_cycles);
    emulator.cpu.write_state(&mut writer);
    emulator.bus.write_state(&mut writer);
    Ok(writer.into_bytes())
}

pub fn decode_state(emulator: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("not a valid ColecoVision save-state");
    }
    let version = reader.read_u32()?;
    if version != VERSION {
        bail!("unsupported ColecoVision save-state version {version}");
    }

    let mut bios_hash = [0; 32];
    reader.read_exact(&mut bios_hash)?;
    if bios_hash != emulator.bios_hash {
        bail!("ColecoVision save-state belongs to a different BIOS");
    }
    let mut cartridge_hash = [0; 32];
    reader.read_exact(&mut cartridge_hash)?;
    if cartridge_hash != emulator.cartridge_hash {
        bail!("ColecoVision save-state belongs to a different cartridge");
    }

    let effective_cycles = reader.read_u64()?;
    let mut cpu = emulator.cpu.clone();
    cpu.read_state(&mut reader)?;
    let mut bus = emulator.bus.clone();
    bus.read_state(&mut reader)?;
    if !reader.is_exhausted() {
        bail!("ColecoVision save-state has unexpected trailing data");
    }
    emulator.effective_cycles = effective_cycles;
    emulator.cpu = cpu;
    emulator.bus = bus;
    emulator.debug.clear_hits();
    emulator.opcode_log.clear();
    emulator.instruction_trace.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BIOS_SIZE;

    fn emulator(cartridge_fill: u8) -> Emulator {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..6].copy_from_slice(&[0x3E, 0x5A, 0x32, 0x00, 0x60, 0x76]);
        let mut cartridge = vec![cartridge_fill; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        Emulator::new(&cartridge, &bios, 48_000).unwrap()
    }

    #[test]
    fn roundtrips_cpu_ram_video_and_timing_state() {
        let mut saved = emulator(0x11);
        saved.step_instruction();
        saved.step_instruction();
        saved.bus_mut().io_write(0xA1, 0x00);
        saved.bus_mut().io_write(0xA1, 0x40);
        saved.bus_mut().io_write(0xA0, 0xA5);
        let state = encode_state(&saved).unwrap();

        let mut restored = emulator(0x11);
        decode_state(&mut restored, &state).unwrap();
        assert_eq!(restored.bus().work_ram()[0], 0x5A);
        assert_eq!(restored.bus().vdp().vram()[0], 0xA5);
        assert_eq!(restored.effective_cycles(), saved.effective_cycles());
        assert_eq!(restored.cpu().regs(), saved.cpu().regs());
    }

    #[test]
    fn rejects_cross_cartridge_and_trailing_data_without_mutation() {
        let mut source = emulator(0x11);
        source.step_instruction();
        let state = encode_state(&source).unwrap();

        let mut other = emulator(0x22);
        let before = encode_state(&other).unwrap();
        assert!(decode_state(&mut other, &state).is_err());
        assert_eq!(encode_state(&other).unwrap(), before);

        let mut trailing = encode_state(&other).unwrap();
        trailing.push(0);
        assert!(decode_state(&mut other, &trailing).is_err());
        assert_eq!(encode_state(&other).unwrap(), before);
    }

    #[test]
    fn truncated_state_is_transactional() {
        let mut emulator = emulator(0x11);
        emulator.step_instruction();
        let state = encode_state(&emulator).unwrap();
        let before = state.clone();

        assert!(decode_state(&mut emulator, &state[..state.len() - 7]).is_err());
        assert_eq!(encode_state(&emulator).unwrap(), before);
    }

    #[test]
    fn state_load_preserves_debug_configuration_and_clears_history() {
        let mut emulator = emulator(0x11);
        emulator.add_breakpoint(4);
        emulator.set_opcode_log_enabled(true);
        emulator.set_instruction_trace_enabled(true);
        emulator.step_instruction();
        let state = encode_state(&emulator).unwrap();

        decode_state(&mut emulator, &state).unwrap();

        assert_eq!(emulator.iter_breakpoints().collect::<Vec<_>>(), vec![4]);
        assert!(emulator.recent_opcodes(1).is_empty());
        assert!(emulator.instruction_trace().is_enabled());
        assert!(emulator.instruction_trace().is_empty());
    }
}
