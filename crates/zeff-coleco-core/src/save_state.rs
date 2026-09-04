use anyhow::{anyhow, bail};
use zeff_emu_common::save_state::{StateReader, StateWriter, ZpstWriter, parse_zpst_envelope};

use crate::{ExpansionHardware, emulator::Emulator};

pub const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBCOLCO\0";
pub const SAVE_STATE_FORMAT_VERSION: u32 = 1;
pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-coleco-determinism-v1";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-coleco-native-state-v1";
const PORTABLE_MAX_BYTES: usize = 512 * 1024;
const PORTABLE_MAX_BLOCKS: usize = 32;
const CORE_TAG: [u8; 4] = *b"CORE";
const IDENTITY_TAG: [u8; 4] = *b"IDEN";
const RAM_TAG: [u8; 4] = *b"WRAM";
const INPUT_TAG: [u8; 4] = *b"INPT";
const VDP_TAG: [u8; 4] = *b"VDP ";
const PSG_TAG: [u8; 4] = *b"PSG ";
const INTERRUPT_TAG: [u8; 4] = *b"INTG";
const PORTABLE_FORMAT_MAJOR: u16 = 1;
const PORTABLE_FORMAT_MINOR: u16 = 0;
const PORTABLE_SYSTEM_TAG: [u8; 4] = *b"COLV";
const PORTABLE_COLECO_SCHEMA_VERSION: u32 = 1;
const PORTABLE_FLAGS: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalStateRestoreOutcome {
    Exact,
    BestEffortPortable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub framebuffer: Box<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateIdentity {
    pub bios_sha256: [u8; 32],
    pub cartridge_sha256: [u8; 32],
    pub expansion_hardware: ExpansionHardware,
}

pub fn inspect_current_native_tas_state_identity(
    data: &[u8],
) -> anyhow::Result<CurrentNativeTasStateIdentity> {
    if data.len() < 76 || data[..8] != SAVE_STATE_MAGIC {
        bail!("TAS requires a native ColecoVision save-state");
    }
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("TAS requires ColecoVision save-state format {SAVE_STATE_FORMAT_VERSION}");
    }
    Ok(CurrentNativeTasStateIdentity {
        bios_sha256: data[12..44].try_into().expect("length checked"),
        cartridge_sha256: data[44..76].try_into().expect("length checked"),
        expansion_hardware: ExpansionHardware::Absent,
    })
}

pub fn encode_state(emulator: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut writer = StateWriter::with_capacity(0x40_000);
    writer.write_bytes(&SAVE_STATE_MAGIC);
    writer.write_u32(SAVE_STATE_FORMAT_VERSION);
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
    if magic != SAVE_STATE_MAGIC {
        bail!("not a valid ColecoVision save-state");
    }
    let version = reader.read_u32()?;
    if version != SAVE_STATE_FORMAT_VERSION {
        bail!("unsupported ColecoVision save-state version {version}");
    }
    if emulator.expansion_hardware() != ExpansionHardware::Absent {
        bail!("ColecoVision save-state topology does not match the emulator");
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

pub fn validate_and_load_current_native_tas_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<CurrentNativeTasStateProjection> {
    inspect_current_native_tas_state_identity(data)?;
    decode_state(emulator, data)?;
    Ok(CurrentNativeTasStateProjection {
        replay_state_bytes: data.to_vec(),
        frame_count: emulator.frame_count(),
        framebuffer: emulator.framebuffer().into(),
    })
}

pub fn encode_external_state(emulator: &Emulator) -> anyhow::Result<Vec<u8>> {
    let native_prefix = encode_state(emulator)?;
    let mut writer = ZpstWriter::new(native_prefix, PORTABLE_MAX_BYTES, PORTABLE_MAX_BLOCKS)?;
    writer.push_block(
        CORE_TAG,
        state_block(|state| {
            state.write_u16(PORTABLE_FORMAT_MAJOR);
            state.write_u16(PORTABLE_FORMAT_MINOR);
            state.write_bytes(&PORTABLE_SYSTEM_TAG);
            state.write_u32(PORTABLE_COLECO_SCHEMA_VERSION);
            state.write_u32(PORTABLE_FLAGS);
            state.write_u64(emulator.effective_cycles);
            emulator.cpu.write_state(state);
        }),
    )?;
    writer.push_block(
        IDENTITY_TAG,
        state_block(|state| {
            state.write_bytes(&emulator.bios_hash);
            state.write_bytes(&emulator.cartridge_hash);
            state.write_u8(0);
        }),
    )?;
    writer.push_block(
        RAM_TAG,
        state_block(|state| emulator.bus.write_portable_ram_state(state)),
    )?;
    writer.push_block(
        INPUT_TAG,
        state_block(|state| emulator.bus.write_portable_input_state(state)),
    )?;
    writer.push_block(
        VDP_TAG,
        state_block(|state| emulator.bus.write_portable_vdp_state(state)),
    )?;
    writer.push_block(
        PSG_TAG,
        state_block(|state| emulator.bus.write_portable_psg_state(state)),
    )?;
    writer.push_block(
        INTERRUPT_TAG,
        state_block(|state| emulator.bus.write_portable_interrupt_state(state)),
    )?;
    writer.finish()
}

pub fn load_external_state(
    emulator: &mut Emulator,
    data: &[u8],
) -> anyhow::Result<ExternalStateRestoreOutcome> {
    if data.starts_with(&SAVE_STATE_MAGIC) {
        if data.len() < 12 {
            bail!("truncated ColecoVision save-state version");
        }
        let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
        if version == SAVE_STATE_FORMAT_VERSION {
            let native_error = match decode_state(emulator, data) {
                Ok(()) => return Ok(ExternalStateRestoreOutcome::Exact),
                Err(error) => error,
            };
            let envelope = match parse_zpst_envelope(data, PORTABLE_MAX_BYTES, PORTABLE_MAX_BLOCKS)
            {
                Ok(envelope) => envelope,
                Err(_) => return Err(native_error),
            };
            if envelope.native_prefix.len() < 12
                || envelope.native_prefix[..8] != SAVE_STATE_MAGIC
                || u32::from_le_bytes(
                    envelope.native_prefix[8..12]
                        .try_into()
                        .expect("length checked"),
                ) != SAVE_STATE_FORMAT_VERSION
            {
                bail!("current ColecoVision state has an invalid ZPST native prefix");
            }
            decode_state(emulator, envelope.native_prefix).map_err(|_| native_error)?;
            return Ok(ExternalStateRestoreOutcome::Exact);
        }
        if version <= SAVE_STATE_FORMAT_VERSION {
            bail!("unsupported ColecoVision save-state version {version}");
        }
    }
    load_portable_fallback(emulator, data)?;
    Ok(ExternalStateRestoreOutcome::BestEffortPortable)
}

fn load_portable_fallback(emulator: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let envelope = parse_zpst_envelope(data, PORTABLE_MAX_BYTES, PORTABLE_MAX_BLOCKS)?;
    validate_portable_prefix(envelope.native_prefix)?;
    let mut core = None;
    let mut identity = None;
    let mut ram = None;
    let mut input = None;
    let mut vdp = None;
    let mut psg = None;
    let mut interrupts = None;
    for block in envelope.blocks {
        let destination = match block.tag {
            CORE_TAG => &mut core,
            IDENTITY_TAG => &mut identity,
            RAM_TAG => &mut ram,
            INPUT_TAG => &mut input,
            VDP_TAG => &mut vdp,
            PSG_TAG => &mut psg,
            INTERRUPT_TAG => &mut interrupts,
            _ => continue,
        };
        if destination.replace(block.payload).is_some() {
            bail!("duplicate required ColecoVision portable block");
        }
    }
    let identity = required_portable_block(identity, "IDEN")?;
    validate_portable_identity(emulator, identity, envelope.native_prefix)?;
    let core = required_portable_block(core, "CORE")?;
    let ram = required_portable_block(ram, "WRAM")?;
    let input = required_portable_block(input, "INPT")?;
    let vdp = required_portable_block(vdp, "VDP ")?;
    let psg = required_portable_block(psg, "PSG ")?;
    let interrupts = required_portable_block(interrupts, "INTG")?;

    let mut core_reader = StateReader::new(core);
    validate_portable_core_header(&mut core_reader)?;
    let effective_cycles = core_reader.read_u64()?;
    let mut cpu = emulator.cpu.clone();
    cpu.read_state(&mut core_reader)?;
    require_exhausted(&core_reader, "CORE")?;
    let mut bus = emulator.bus.clone();
    read_portable_bus_block(&mut bus, ram, "WRAM", |bus, reader| {
        bus.read_portable_ram_state(reader)
    })?;
    read_portable_bus_block(&mut bus, input, "INPT", |bus, reader| {
        bus.read_portable_input_state(reader)
    })?;
    read_portable_bus_block(&mut bus, vdp, "VDP ", |bus, reader| {
        bus.read_portable_vdp_state(reader)
    })?;
    read_portable_bus_block(&mut bus, psg, "PSG ", |bus, reader| {
        bus.read_portable_psg_state(reader)
    })?;
    read_portable_bus_block(&mut bus, interrupts, "INTG", |bus, reader| {
        bus.read_portable_interrupt_state(reader)
    })?;
    emulator.effective_cycles = effective_cycles;
    emulator.cpu = cpu;
    emulator.bus = bus;
    emulator.debug.clear_hits();
    emulator.opcode_log.clear();
    emulator.instruction_trace.clear();
    Ok(())
}

fn validate_portable_core_header(reader: &mut StateReader<'_>) -> anyhow::Result<()> {
    let major = reader.read_u16()?;
    let minor = reader.read_u16()?;
    let mut system = [0; 4];
    reader.read_exact(&mut system)?;
    let schema = reader.read_u32()?;
    let flags = reader.read_u32()?;
    if major != PORTABLE_FORMAT_MAJOR
        || minor != PORTABLE_FORMAT_MINOR
        || system != PORTABLE_SYSTEM_TAG
        || schema != PORTABLE_COLECO_SCHEMA_VERSION
        || flags != PORTABLE_FLAGS
    {
        bail!("unsupported ColecoVision portable CORE format");
    }
    Ok(())
}

fn state_block(write: impl FnOnce(&mut StateWriter)) -> Vec<u8> {
    let mut writer = StateWriter::new();
    write(&mut writer);
    writer.into_bytes()
}

fn required_portable_block<'a>(block: Option<&'a [u8]>, tag: &str) -> anyhow::Result<&'a [u8]> {
    block.ok_or_else(|| anyhow!("missing required ColecoVision portable {tag} block"))
}

fn require_exhausted(reader: &StateReader<'_>, tag: &str) -> anyhow::Result<()> {
    if reader.is_exhausted() {
        Ok(())
    } else {
        bail!("ColecoVision portable {tag} block has trailing data")
    }
}

fn read_portable_bus_block(
    bus: &mut crate::bus::Bus,
    bytes: &[u8],
    tag: &str,
    read: impl FnOnce(&mut crate::bus::Bus, &mut StateReader<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut reader = StateReader::new(bytes);
    read(bus, &mut reader)?;
    require_exhausted(&reader, tag)
}

fn validate_portable_prefix(prefix: &[u8]) -> anyhow::Result<()> {
    if prefix.len() < 76 || prefix[..8] != SAVE_STATE_MAGIC {
        bail!("ZPST native prefix is not a ColecoVision save-state");
    }
    let version = u32::from_le_bytes(prefix[8..12].try_into().expect("length checked"));
    if version <= SAVE_STATE_FORMAT_VERSION {
        bail!("ZPST fallback requires an unsupported future ColecoVision version");
    }
    Ok(())
}

fn validate_portable_identity(
    emulator: &Emulator,
    bytes: &[u8],
    prefix: &[u8],
) -> anyhow::Result<()> {
    let mut reader = StateReader::new(bytes);
    let mut bios_hash = [0; 32];
    let mut cartridge_hash = [0; 32];
    reader.read_exact(&mut bios_hash)?;
    reader.read_exact(&mut cartridge_hash)?;
    let expansion = reader.read_u8()?;
    require_exhausted(&reader, "IDEN")?;
    if bios_hash != emulator.bios_hash || cartridge_hash != emulator.cartridge_hash {
        bail!("ColecoVision portable state belongs to a different BIOS or cartridge");
    }
    if expansion != 0 || emulator.expansion_hardware() != ExpansionHardware::Absent {
        bail!("ColecoVision portable state topology does not match the emulator");
    }
    if prefix[12..44] != bios_hash || prefix[44..76] != cartridge_hash {
        bail!("ZPST native prefix identity does not match its portable payload");
    }
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

    fn rebuild_external(
        external: &[u8],
        mutate: impl FnOnce(&mut Vec<([u8; 4], Vec<u8>)>),
    ) -> Vec<u8> {
        let envelope =
            parse_zpst_envelope(external, PORTABLE_MAX_BYTES, PORTABLE_MAX_BLOCKS).unwrap();
        let mut prefix = envelope.native_prefix.to_vec();
        prefix[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());
        let mut blocks = envelope
            .blocks
            .into_iter()
            .map(|block| (block.tag, block.payload.to_vec()))
            .collect();
        mutate(&mut blocks);
        let mut writer = ZpstWriter::new(prefix, PORTABLE_MAX_BYTES, PORTABLE_MAX_BLOCKS).unwrap();
        for (tag, payload) in blocks {
            writer.push_block(tag, payload).unwrap();
        }
        writer.finish().unwrap()
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

    #[test]
    fn current_native_tas_state_restores_exact_frame_output() {
        let mut source = emulator(0x11);
        source.step_frame();
        source.bus_mut().vdp_mut().write_register(7, 0x02);
        source.step_frame();
        let state = source.save_state().unwrap();
        assert_eq!(
            inspect_current_native_tas_state_identity(&state)
                .unwrap()
                .expansion_hardware,
            ExpansionHardware::Absent
        );

        let mut target = emulator(0x11);
        let projection = validate_and_load_current_native_tas_state(&mut target, &state).unwrap();

        assert_eq!(projection.replay_state_bytes, state);
        assert_eq!(projection.frame_count, source.frame_count());
        assert_eq!(projection.framebuffer.as_ref(), source.framebuffer());
        assert_eq!(target.save_state().unwrap(), state);
        assert_eq!(target.framebuffer(), source.framebuffer());
    }

    #[test]
    fn current_native_tas_state_rejects_wrong_schema_or_identity_transactionally() {
        let source = emulator(0x11);
        let current = source.save_state().unwrap();
        let mut target = emulator(0x11);
        target.step_instruction();
        let before = target.save_state().unwrap();

        let mut wrong_version = current.clone();
        wrong_version[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());
        let mut wrong_magic = current.clone();
        wrong_magic[0] ^= 1;
        let wrong_cartridge = emulator(0x22).save_state().unwrap();
        for invalid in [&wrong_version, &wrong_magic, &wrong_cartridge] {
            assert!(validate_and_load_current_native_tas_state(&mut target, invalid).is_err());
            assert_eq!(target.save_state().unwrap(), before);
        }
    }

    #[test]
    fn external_state_restores_a_complete_portable_snapshot() {
        let mut source = emulator(0x11);
        source.set_controller(
            0,
            crate::input::StandardController {
                up: true,
                left_button: true,
                keypad: Some(crate::input::KeypadKey::Pound),
                ..crate::input::StandardController::default()
            },
        );
        source.bus_mut().io_write(0xC0, 0);
        source.bus_mut().io_write(0xE0, 0x87);
        source.bus_mut().io_write(0xE0, 0x12);
        source.bus_mut().io_write(0xA1, 0x00);
        source.bus_mut().io_write(0xA1, 0x40);
        source.bus_mut().io_write(0xA0, 0xA5);
        source.step_frame();
        let native = source.save_state().unwrap();
        let external = source.encode_external_state().unwrap();
        assert!(external.len() <= PORTABLE_MAX_BYTES);
        assert_eq!(&external[..native.len()], native);

        let mut target = emulator(0x11);
        target.set_sample_rate(32_000);
        target.set_audio_generation_enabled(false);
        target.set_audio_muted(true);
        assert_eq!(
            target.load_external_state(&external).unwrap(),
            ExternalStateRestoreOutcome::Exact
        );
        assert_eq!(target.save_state().unwrap(), native);
        assert!(target.drain_audio_samples().is_empty());
    }

    #[test]
    fn external_state_prefers_exact_native_and_rejects_supported_native_appendix() {
        let mut source = emulator(0x11);
        source.step_frame();
        let native = source.save_state().unwrap();
        let mut target = emulator(0x11);
        assert_eq!(
            target.load_external_state(&native).unwrap(),
            ExternalStateRestoreOutcome::Exact
        );

        let disguised_portable = rebuild_external(&source.encode_external_state().unwrap(), |_| {});
        assert_eq!(
            target.load_external_state(&disguised_portable).unwrap(),
            ExternalStateRestoreOutcome::BestEffortPortable
        );

        let mut supported_native_appendix = source.encode_external_state().unwrap();
        supported_native_appendix.push(0);
        let before = target.save_state().unwrap();
        assert!(
            target
                .load_external_state(&supported_native_appendix)
                .is_err()
        );
        assert_eq!(target.save_state().unwrap(), before);

        let mut corrupted_native_prefix = source.encode_external_state().unwrap();
        corrupted_native_prefix[12] ^= 1;
        assert!(
            target
                .load_external_state(&corrupted_native_prefix)
                .is_err()
        );
        assert_eq!(target.save_state().unwrap(), before);
    }

    #[test]
    fn malformed_or_duplicate_portable_blocks_are_atomic() {
        let source = emulator(0x11);
        let external = source.encode_external_state().unwrap();
        let mut target = emulator(0x11);
        target.step_instruction();
        let before = target.save_state().unwrap();

        let mut tampered = external.clone();
        tampered[8..12].copy_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());
        let auth_payload = tampered.len() - 8 - 8 - 32 + 8;
        tampered[auth_payload] ^= 1;
        assert!(target.load_external_state(&tampered).is_err());
        assert_eq!(target.save_state().unwrap(), before);

        let duplicate = rebuild_external(&external, |blocks| {
            blocks.push((CORE_TAG, vec![0]));
        });
        assert!(target.load_external_state(&duplicate).is_err());
        assert_eq!(target.save_state().unwrap(), before);
    }

    #[test]
    fn future_portable_state_ignores_unknown_blocks() {
        let source = emulator(0x11);
        let external = source.encode_external_state().unwrap();
        let future = rebuild_external(&external, |blocks| {
            blocks.push((*b"FUTR", vec![1, 2, 3]));
        });

        let mut target = emulator(0x11);
        assert_eq!(
            target.load_external_state(&future).unwrap(),
            ExternalStateRestoreOutcome::BestEffortPortable
        );
        assert_eq!(target.save_state().unwrap(), source.save_state().unwrap());
    }

    #[test]
    fn portable_core_header_is_explicit_and_strict() {
        let source = emulator(0x11);
        let external = source.encode_external_state().unwrap();
        for offset in [0, 2, 4, 8, 12] {
            let invalid = rebuild_external(&external, |blocks| {
                let core = blocks.iter_mut().find(|(tag, _)| *tag == CORE_TAG).unwrap();
                core.1[offset] ^= 1;
            });
            let mut target = emulator(0x11);
            let before = target.save_state().unwrap();
            assert!(target.load_external_state(&invalid).is_err());
            assert_eq!(target.save_state().unwrap(), before);
        }
    }
}
