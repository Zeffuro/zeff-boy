use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{PceConsoleWiring, PceHardwareTopology, PceHuCardBoard, PceMachine, PsgRevision};

const MAGIC: &[u8; 8] = b"ZBPCE\0\0\0";
const VERSION: u32 = 1;
const CONTENT_HUCARD: u8 = 0;
const CONTENT_CD: u8 = 1;
const MAX_BODY_SIZE: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct PceV1Identity {
    pub board: PceHuCardBoard,
    pub topology: PceHardwareTopology,
    pub wiring: PceConsoleWiring,
    pub psg_revision: PsgRevision,
    pub is_cd: bool,
    pub has_arcade_card: bool,
}

pub fn encode_state(machine: &PceMachine) -> anyhow::Result<Vec<u8>> {
    machine.validate_v1_encode_state()?;

    let mut writer = StateWriter::with_capacity(4_500_000);
    writer.write_bytes(MAGIC);
    writer.write_u32(VERSION);
    let cdrom2 = machine.devices().cdrom2();
    writer.write_u8(if cdrom2.is_some() {
        CONTENT_CD
    } else {
        CONTENT_HUCARD
    });
    writer.write_bytes(&rom_hash(machine));
    writer.write_u8(board_to_tag(machine.hucard_board()));
    writer.write_u8(topology_to_tag(machine.hardware_topology()));
    writer.write_u8(wiring_to_tag(machine.devices().console_wiring()));
    writer.write_u8(psg_revision_to_tag(machine.devices().psg().revision()));
    writer.write_bool(machine.devices().arcade_card().is_some());
    if let Some(cdrom2) = cdrom2 {
        writer.write_bytes(&cdrom2.disc().content_hash());
    }
    write_section(&mut writer, |section| machine.write_v1_state(section));
    Ok(writer.into_bytes())
}

pub fn decode_state(machine: &mut PceMachine, data: &[u8]) -> anyhow::Result<()> {
    machine.validate_v1_state_target()?;

    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("not a valid PC Engine save-state");
    }
    let version = reader.read_u32()?;
    if version != VERSION {
        bail!("unsupported PC Engine save-state version {version}");
    }
    let saved_is_cd = match reader.read_u8()? {
        CONTENT_HUCARD => false,
        CONTENT_CD => true,
        tag => bail!("invalid PC Engine content-kind tag in save-state: {tag}"),
    };
    let target_is_cd = machine.devices().cdrom2().is_some();
    if saved_is_cd != target_is_cd {
        bail!("PC Engine save-state media kind does not match the running machine");
    }

    let mut saved_hash = [0; 32];
    reader.read_exact(&mut saved_hash)?;
    if saved_hash != rom_hash(machine) {
        let media = if saved_is_cd { "System Card" } else { "HuCard" };
        bail!("PC Engine save-state belongs to a different {media} ROM");
    }

    let board = tag_to_board(reader.read_u8()?)?;
    if board != machine.hucard_board() {
        bail!("PC Engine save-state HuCard board does not match the loaded cartridge");
    }
    let topology = tag_to_topology(reader.read_u8()?)?;
    if topology != machine.hardware_topology() {
        bail!("PC Engine save-state hardware topology does not match the running machine");
    }
    let wiring = tag_to_wiring(reader.read_u8()?)?;
    if wiring != machine.devices().console_wiring() {
        bail!("PC Engine save-state console wiring does not match the running machine");
    }
    let psg_revision = tag_to_psg_revision(reader.read_u8()?)?;
    if psg_revision != machine.devices().psg().revision() {
        bail!("PC Engine save-state PSG revision does not match the running machine");
    }
    let has_arcade_card = reader.read_bool()?;
    if has_arcade_card != machine.devices().arcade_card().is_some() {
        bail!("PC Engine save-state Arcade Card topology does not match the running machine");
    }
    if saved_is_cd {
        let mut saved_disc_hash = [0; 32];
        reader.read_exact(&mut saved_disc_hash)?;
        if saved_disc_hash
            != machine
                .devices()
                .cdrom2()
                .expect("CD state target has CD hardware")
                .disc()
                .content_hash()
        {
            bail!("PC Engine save-state belongs to different CD media");
        }
    }

    let body = reader.read_vec(MAX_BODY_SIZE)?;
    if !reader.is_exhausted() {
        bail!("PC Engine save-state has unexpected trailing data");
    }
    machine
        .replace_from_v1_state(
            &body,
            PceV1Identity {
                board,
                topology,
                wiring,
                psg_revision,
                is_cd: saved_is_cd,
                has_arcade_card,
            },
        )
        .context("invalid PC Engine save-state payload")
}

pub(super) fn write_section(writer: &mut StateWriter, write: impl FnOnce(&mut StateWriter)) {
    let mut section = StateWriter::new();
    write(&mut section);
    writer.write_vec(&section.into_bytes());
}

pub(super) fn read_section(
    reader: &mut StateReader<'_>,
    max_len: usize,
    label: &str,
    read: impl FnOnce(&mut StateReader<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let data = reader.read_vec(max_len)?;
    let mut section = StateReader::new(&data);
    read(&mut section).with_context(|| format!("invalid PC Engine {label} section"))?;
    if !section.is_exhausted() {
        bail!("PC Engine {label} section has unexpected trailing data");
    }
    Ok(())
}

fn rom_hash(machine: &PceMachine) -> [u8; 32] {
    Sha256::digest(machine.hucard_rom()).into()
}

const fn board_to_tag(board: PceHuCardBoard) -> u8 {
    match board {
        PceHuCardBoard::Plain => 0,
        PceHuCardBoard::Sf2Ce => 1,
        PceHuCardBoard::Populous => 2,
        PceHuCardBoard::SystemCardV1V2 => 3,
        PceHuCardBoard::SystemCardV3 => 4,
    }
}

fn tag_to_board(tag: u8) -> anyhow::Result<PceHuCardBoard> {
    Ok(match tag {
        0 => PceHuCardBoard::Plain,
        1 => PceHuCardBoard::Sf2Ce,
        2 => PceHuCardBoard::Populous,
        3 => PceHuCardBoard::SystemCardV1V2,
        4 => PceHuCardBoard::SystemCardV3,
        _ => bail!("invalid PC Engine HuCard-board tag in save-state: {tag}"),
    })
}

const fn topology_to_tag(topology: PceHardwareTopology) -> u8 {
    match topology {
        PceHardwareTopology::Base => 0,
        PceHardwareTopology::SuperGrafx => 1,
    }
}

fn tag_to_topology(tag: u8) -> anyhow::Result<PceHardwareTopology> {
    Ok(match tag {
        0 => PceHardwareTopology::Base,
        1 => PceHardwareTopology::SuperGrafx,
        _ => bail!("invalid PC Engine hardware-topology tag in save-state: {tag}"),
    })
}

const fn wiring_to_tag(wiring: PceConsoleWiring) -> u8 {
    match wiring {
        PceConsoleWiring::PcEngine => 0,
        PceConsoleWiring::TurboGrafx16 => 1,
    }
}

fn tag_to_wiring(tag: u8) -> anyhow::Result<PceConsoleWiring> {
    Ok(match tag {
        0 => PceConsoleWiring::PcEngine,
        1 => PceConsoleWiring::TurboGrafx16,
        _ => bail!("invalid PC Engine console-wiring tag in save-state: {tag}"),
    })
}

const fn psg_revision_to_tag(revision: PsgRevision) -> u8 {
    match revision {
        PsgRevision::HuC6280 => 0,
        PsgRevision::HuC6280A => 1,
    }
}

fn tag_to_psg_revision(tag: u8) -> anyhow::Result<PsgRevision> {
    Ok(match tag {
        0 => PsgRevision::HuC6280,
        1 => PsgRevision::HuC6280A,
        _ => bail!("invalid PC Engine PSG-revision tag in save-state: {tag}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cpu::VdcPort;
    use crate::hardware::{
        CD_USER_SECTOR_BYTES, CDROM2_ADPCM_RAM_LEN, CDROM2_BRAM_LEN, CDROM2_REGISTER_START,
        CDROM2_WORK_RAM_LEN, CdAudioStatus, CdDisc, CdScsiPhase, CdTrack, CdTrackMode,
        ControllerDevice, ControllerPort, HuC6270, PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS,
        PROVISIONAL_CDROM2_PHASE_TICKS, PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE,
        PceCartridgeDescriptor, PceCartridgeHardware, PceMouse, PsgPort, SUPERGRAFX_WORK_RAM_LEN,
        VcePort, VdcRegister, VpcPort,
    };

    const VERSION_OFFSET: usize = 8;
    const CONTENT_OFFSET: usize = VERSION_OFFSET + 4;
    const BOARD_OFFSET: usize = CONTENT_OFFSET + 1 + 32;
    const TOPOLOGY_OFFSET: usize = BOARD_OFFSET + 1;
    const ARCADE_CARD_OFFSET: usize = TOPOLOGY_OFFSET + 3;
    const BODY_LEN_OFFSET: usize = ARCADE_CARD_OFFSET + 1;

    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0xEA; 0x2000];
        rom[..10].copy_from_slice(&[0xA9, 0xF8, 0x53, 0x02, 0xEE, 0x00, 0x20, 0x4C, 0x04, 0x00]);
        rom[0x1FFE] = 0;
        rom[0x1FFF] = 0;
        rom
    }

    fn test_machine(rom: Vec<u8>) -> PceMachine {
        let mut machine = PceMachine::new(rom).unwrap();
        machine.set_sample_generation_enabled(false);
        machine
    }

    fn test_supergrafx_machine(rom: Vec<u8>) -> PceMachine {
        let descriptor = PceCartridgeDescriptor::default()
            .with_required_hardware(PceCartridgeHardware::SuperGrafx);
        let mut machine = PceMachine::with_cartridge(rom, descriptor).unwrap();
        machine.set_sample_generation_enabled(false);
        machine
    }

    fn run_boundaries(machine: &mut PceMachine, count: usize) {
        for _ in 0..count {
            machine.step_boundary().unwrap();
        }
    }

    fn body_section_payload(bytes: &[u8], section_index: usize) -> std::ops::Range<usize> {
        let body_len_offset = BODY_LEN_OFFSET
            + if bytes[CONTENT_OFFSET] == CONTENT_CD {
                32
            } else {
                0
            };
        let mut offset = body_len_offset + size_of::<u32>();
        for _ in 0..section_index {
            let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += size_of::<u32>() + len;
        }
        let len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset + size_of::<u32>()..offset + size_of::<u32>() + len
    }

    fn write_vdc_register(machine: &mut PceMachine, register: VdcRegister, value: u16) {
        write_vdc_register_on(machine.devices_mut().vdc_mut(), register, value);
    }

    fn test_system_card_rom() -> Vec<u8> {
        let mut rom = vec![0xEA; super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
        rom[0x1FFE] = 0;
        rom[0x1FFF] = 0;
        rom
    }

    fn test_cd_disc() -> CdDisc {
        test_cd_disc_with_seed(0)
    }

    fn test_cd_disc_with_seed(seed: u8) -> CdDisc {
        let mut bytes = vec![0; 2 * CD_USER_SECTOR_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_add(seed);
        }
        let track =
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, bytes).unwrap();
        CdDisc::new(vec![track]).unwrap()
    }

    fn test_audio_disc() -> CdDisc {
        let mut raw = vec![0; 4 * 2_352];
        for (index, frame) in raw.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let left = 0x1000_i16.wrapping_add(index as i16);
            frame[..2].copy_from_slice(&left.to_le_bytes());
            frame[2..].copy_from_slice(&(-left).to_le_bytes());
        }
        let track = CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, raw).unwrap();
        CdDisc::new(vec![track]).unwrap()
    }

    fn select_cd(cd: &mut super::super::CdRom2) {
        cd.write_physical(CDROM2_REGISTER_START, 0);
        cd.advance_master_ticks(super::super::PROVISIONAL_CDROM2_SELECTION_TICKS);
        assert_eq!(cd.phase(), CdScsiPhase::Command);
    }

    fn send_cd_command(cd: &mut super::super::CdRom2, command: &[u8]) {
        for (index, &byte) in command.iter().enumerate() {
            cd.write_physical(CDROM2_REGISTER_START + 1, byte);
            cd.write_physical(CDROM2_REGISTER_START + 2, 0x80);
            cd.write_physical(CDROM2_REGISTER_START + 2, 0);
            if index + 1 != command.len() {
                cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
            }
        }
    }

    fn cd_machine(disc: CdDisc, generation_enabled: bool) -> PceMachine {
        let mut machine = PceMachine::with_cdrom2(test_system_card_rom(), disc).unwrap();
        machine.set_sample_rate(48_000);
        machine.set_sample_generation_enabled(generation_enabled);
        machine
    }

    fn arcade_card_machine(disc: CdDisc) -> PceMachine {
        let mut machine = PceMachine::with_cdrom2_system_card_controller_and_arcade_card(
            test_system_card_rom(),
            PceHuCardBoard::SystemCardV3,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
            true,
        )
        .unwrap();
        machine.set_sample_generation_enabled(false);
        machine
    }

    fn write_vdc_register_on(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
        vdc.write_port(VdcPort::SelectOrStatus, register as u8);
        vdc.write_port(VdcPort::DataLow, value as u8);
        vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
    }

    fn mutate_hardware_state(machine: &mut PceMachine) {
        machine.devices_mut().vdc_mut().vram_mut()[0x100] = 0xA55A;
        write_vdc_register(machine, VdcRegister::DmaSource, 0x0100);
        write_vdc_register(machine, VdcRegister::DmaDestination, 0x0200);
        write_vdc_register(machine, VdcRegister::DmaLength, 0x0007);
        machine.devices_mut().vdc_mut().queue_vram_dma();

        let vce = machine.devices_mut().vce_mut();
        vce.write_port(VcePort::from_offset(2), 7);
        vce.write_port(VcePort::from_offset(3), 1);
        vce.write_port(VcePort::from_offset(4), 0x5A);
        vce.write_port(VcePort::from_offset(5), 1);

        let psg = machine.devices_mut().psg_mut();
        psg.write_port(PsgPort::from_offset(0), 2);
        psg.write_port(PsgPort::from_offset(2), 0x34);
        psg.write_port(PsgPort::from_offset(3), 0x02);
        psg.write_port(PsgPort::from_offset(4), 0x9F);
        psg.write_port(PsgPort::from_offset(6), 0x17);

        let mut mouse = PceMouse::new();
        mouse.accumulate_motion(23, -11);
        machine
            .devices_mut()
            .set_controller_device(ControllerDevice::Mouse(mouse));
        machine
            .devices_mut()
            .controller_mut()
            .write_lines(false, false);
        machine
            .devices_mut()
            .controller_mut()
            .write_lines(true, true);
    }

    fn configure_audible_psg(machine: &mut PceMachine) {
        let psg = machine.devices_mut().psg_mut();
        psg.write_port(PsgPort::from_offset(0), 0);
        psg.write_port(PsgPort::from_offset(1), 0xFF);
        psg.write_port(PsgPort::from_offset(5), 0xFF);
        psg.write_port(PsgPort::from_offset(4), 0xDF);
        psg.write_port(PsgPort::from_offset(6), 0x1F);
    }

    #[test]
    fn roundtrips_full_base_state_and_continues_deterministically() {
        let rom = test_rom();
        let mut saved = test_machine(rom.clone());
        run_boundaries(&mut saved, 173);
        assert_ne!(saved.vce_line_accumulator(), 0);
        assert_ne!(saved.work_ram()[0], 0);
        mutate_hardware_state(&mut saved);
        assert!(saved.devices().vdc().pending_vram_dma().is_some());

        let bytes = encode_state(&saved).unwrap();
        let mut restored = test_machine(rom);
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), bytes);

        run_boundaries(&mut saved, 257);
        run_boundaries(&mut restored, 257);
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
    }

    #[test]
    fn enabled_audio_roundtrips_bytes_and_pcm_continuation_exactly() {
        let rom = test_rom();
        let mut saved = PceMachine::new(rom.clone()).unwrap();
        configure_audible_psg(&mut saved);
        run_boundaries(&mut saved, 20_000);
        assert!(
            saved
                .devices()
                .psg()
                .debug_snapshot()
                .buffered_sample_frames
                > 0
        );

        let bytes = encode_state(&saved).unwrap();
        let mut restored = PceMachine::new(rom).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), bytes);

        let mut saved_pcm = Vec::new();
        let mut restored_pcm = Vec::new();
        saved.drain_audio_samples_into(&mut saved_pcm);
        restored.drain_audio_samples_into(&mut restored_pcm);
        assert!(!saved_pcm.is_empty());
        assert_eq!(restored_pcm, saved_pcm);

        run_boundaries(&mut saved, 5_000);
        run_boundaries(&mut restored, 5_000);
        saved.drain_audio_samples_into(&mut saved_pcm);
        restored.drain_audio_samples_into(&mut restored_pcm);
        assert!(!saved_pcm.is_empty());
        assert_eq!(restored_pcm, saved_pcm);
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
    }

    #[test]
    fn audio_load_requires_rate_and_preserves_destination_host_controls() {
        let rom = test_rom();
        let mut saved = PceMachine::new(rom.clone()).unwrap();
        configure_audible_psg(&mut saved);
        run_boundaries(&mut saved, 1_000);
        let bytes = encode_state(&saved).unwrap();

        let mut wrong_rate = PceMachine::new(rom.clone()).unwrap();
        wrong_rate.set_sample_rate(48_000);
        let wrong_rate_before = encode_state(&wrong_rate).unwrap();
        let error = decode_state(&mut wrong_rate, &bytes).unwrap_err();
        assert!(format!("{error:#}").contains("sample rate mismatch"));
        assert_eq!(encode_state(&wrong_rate).unwrap(), wrong_rate_before);

        let mutes = [true, false, true, false, true, false];
        let mut host_disabled = PceMachine::new(rom).unwrap();
        host_disabled.set_sample_generation_enabled(false);
        host_disabled.set_channel_mutes(&mutes);
        decode_state(&mut host_disabled, &bytes).unwrap();
        let snapshot = host_disabled.devices().psg().debug_snapshot();
        assert!(!snapshot.sample_generation_enabled);
        assert_eq!(snapshot.channel_mutes, mutes);
        assert_eq!(snapshot.buffered_sample_frames, 0);

        let disabled_bytes = encode_state(&test_machine(test_rom())).unwrap();
        let mut host_enabled = PceMachine::new(test_rom()).unwrap();
        host_enabled.set_channel_mutes(&mutes);
        decode_state(&mut host_enabled, &disabled_bytes).unwrap();
        let snapshot = host_enabled.devices().psg().debug_snapshot();
        assert!(snapshot.sample_generation_enabled);
        assert_eq!(snapshot.channel_mutes, mutes);
    }

    #[test]
    fn rejects_wrong_identity_truncation_and_trailing_data_without_mutation() {
        let rom = test_rom();
        let mut saved = test_machine(rom.clone());
        run_boundaries(&mut saved, 19);
        let bytes = encode_state(&saved).unwrap();

        let mut target = test_machine(rom.clone());
        run_boundaries(&mut target, 7);
        let target_before = encode_state(&target).unwrap();

        let mut other_rom = rom;
        other_rom[0x100] ^= 0xFF;
        let mut wrong_rom = test_machine(other_rom);
        assert!(
            decode_state(&mut wrong_rom, &bytes)
                .unwrap_err()
                .to_string()
                .contains("different HuCard ROM")
        );

        let mut wrong_board = bytes.clone();
        wrong_board[BOARD_OFFSET] = 1;
        assert!(
            decode_state(&mut target, &wrong_board)
                .unwrap_err()
                .to_string()
                .contains("board")
        );

        let mut wrong_topology = bytes.clone();
        wrong_topology[TOPOLOGY_OFFSET] = 1;
        assert!(
            decode_state(&mut target, &wrong_topology)
                .unwrap_err()
                .to_string()
                .contains("topology")
        );

        let mut wrong_version = bytes.clone();
        wrong_version[VERSION_OFFSET..VERSION_OFFSET + 4].copy_from_slice(&2_u32.to_le_bytes());
        assert!(decode_state(&mut target, &wrong_version).is_err());

        let mut wrong_magic = bytes.clone();
        wrong_magic[0] ^= 0xFF;
        assert!(decode_state(&mut target, &wrong_magic).is_err());

        let mut cd_content = bytes.clone();
        cd_content[CONTENT_OFFSET] = 1;
        assert!(
            decode_state(&mut target, &cd_content)
                .unwrap_err()
                .to_string()
                .contains("media kind")
        );

        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(decode_state(&mut target, &truncated).is_err());

        let mut trailing = bytes;
        let body_len = u32::from_le_bytes(
            trailing[BODY_LEN_OFFSET..BODY_LEN_OFFSET + 4]
                .try_into()
                .unwrap(),
        );
        trailing[BODY_LEN_OFFSET..BODY_LEN_OFFSET + 4]
            .copy_from_slice(&(body_len + 1).to_le_bytes());
        trailing.push(0);
        assert!(
            decode_state(&mut target, &trailing)
                .unwrap_err()
                .to_string()
                .contains("payload")
        );
        assert_eq!(encode_state(&target).unwrap(), target_before);
    }

    #[test]
    fn rejects_wrong_wiring_and_psg_revision() {
        let rom = test_rom();
        let saved = test_machine(rom.clone());
        let bytes = encode_state(&saved).unwrap();

        let descriptor =
            PceCartridgeDescriptor::default().with_console_wiring(PceConsoleWiring::TurboGrafx16);
        let mut wrong_wiring = PceMachine::with_cartridge(rom.clone(), descriptor).unwrap();
        wrong_wiring.set_sample_generation_enabled(false);
        assert!(
            decode_state(&mut wrong_wiring, &bytes)
                .unwrap_err()
                .to_string()
                .contains("wiring")
        );

        let mut wrong_psg = PceMachine::with_psg_revision(rom, PsgRevision::HuC6280A).unwrap();
        wrong_psg.set_sample_generation_enabled(false);
        assert!(
            decode_state(&mut wrong_psg, &bytes)
                .unwrap_err()
                .to_string()
                .contains("PSG revision")
        );
    }

    #[test]
    fn roundtrips_sf2_bank_and_hucard_ram_boards() {
        let mut sf2_rom = vec![0xEA; super::super::SF2_CE_HUCARD_IMAGE_LEN];
        sf2_rom[..6].copy_from_slice(&[0xA9, 0x00, 0x8D, 0xF3, 0x1F, 0xEA]);
        sf2_rom[0x1FFE] = 0;
        sf2_rom[0x1FFF] = 0;
        let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Sf2Ce);
        let mut sf2 = PceMachine::with_cartridge(sf2_rom.clone(), descriptor).unwrap();
        sf2.set_sample_generation_enabled(false);
        run_boundaries(&mut sf2, 2);
        let mapping_token = sf2.rom_mapping_token();
        let sf2_bytes = encode_state(&sf2).unwrap();
        let mut restored_sf2 = PceMachine::with_cartridge(sf2_rom, descriptor).unwrap();
        restored_sf2.set_sample_generation_enabled(false);
        decode_state(&mut restored_sf2, &sf2_bytes).unwrap();
        assert_eq!(restored_sf2.rom_mapping_token(), mapping_token);

        let mut populous_rom = vec![0xEA; super::super::POPULOUS_HUCARD_IMAGE_LEN];
        populous_rom[..12].copy_from_slice(&[
            0xA9, 0x40, 0x53, 0x02, 0xA9, 0xA5, 0x8D, 0x00, 0x20, 0x4C, 0x09, 0x00,
        ]);
        populous_rom[0x1FFE] = 0;
        populous_rom[0x1FFF] = 0;
        let descriptor =
            PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Populous);
        let mut populous = PceMachine::with_cartridge(populous_rom.clone(), descriptor).unwrap();
        populous.set_sample_generation_enabled(false);
        run_boundaries(&mut populous, 4);
        assert_eq!(populous.hucard_ram().unwrap()[0], 0xA5);
        let populous_bytes = encode_state(&populous).unwrap();
        let mut restored_populous = PceMachine::with_cartridge(populous_rom, descriptor).unwrap();
        restored_populous.set_sample_generation_enabled(false);
        decode_state(&mut restored_populous, &populous_bytes).unwrap();
        assert_eq!(restored_populous.hucard_ram().unwrap()[0], 0xA5);

        let mut system_card_rom = vec![0xEA; super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
        system_card_rom[..12].copy_from_slice(&[
            0xA9, 0x68, 0x53, 0x02, 0xA9, 0x3C, 0x8D, 0x00, 0x20, 0x4C, 0x09, 0x00,
        ]);
        system_card_rom[0x1FFE] = 0;
        system_card_rom[0x1FFF] = 0;
        let descriptor =
            PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::SystemCardV3);
        let mut system_card =
            PceMachine::with_cartridge(system_card_rom.clone(), descriptor).unwrap();
        system_card.set_sample_generation_enabled(false);
        run_boundaries(&mut system_card, 4);
        assert_eq!(system_card.debug_peek_physical8(0x0D_0000), 0x3C);
        let system_card_bytes = encode_state(&system_card).unwrap();
        let mut restored_system_card =
            PceMachine::with_cartridge(system_card_rom, descriptor).unwrap();
        restored_system_card.set_sample_generation_enabled(false);
        decode_state(&mut restored_system_card, &system_card_bytes).unwrap();
        assert_eq!(restored_system_card.debug_peek_physical8(0x0D_0000), 0x3C);
    }

    #[test]
    fn roundtrips_connected_memory_base_ram_and_protocol_state() {
        let rom = test_rom();
        let mut saved = test_machine(rom.clone());
        let mut ram = vec![0; super::super::MEMORY_BASE128_RAM_LEN];
        for (index, byte) in ram.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_mul(29);
        }
        let controller = saved.devices_mut().controller_mut();
        controller.memory_base128_mut().load_ram(&ram).unwrap();
        controller.set_memory_base128_connected(true);
        for bit in [false, false, false, true, false, true, false, true] {
            controller.write_lines(bit, false);
            controller.write_lines(bit, true);
        }
        controller.write_lines(false, false);
        controller.write_lines(false, true);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = test_machine(rom);
        decode_state(&mut restored, &bytes).unwrap();
        let memory_base = restored.devices().controller().memory_base128();
        assert!(memory_base.is_connected());
        assert_eq!(memory_base.ram(), ram);
        assert_eq!(
            memory_base.debug_snapshot().phase,
            super::super::MemoryBase128Phase::IdentifySecond
        );
        assert_eq!(encode_state(&restored).unwrap(), bytes);
    }

    #[test]
    fn supergrafx_roundtrips_dual_video_dma_and_continues_mid_frame() {
        let rom = test_rom();
        let mut saved = test_supergrafx_machine(rom.clone());

        saved.cpu_mut().cpu_mut().set_mapping_register(2, 0xFB);
        saved.debug_write_cpu8(0x4005, 0xA5);
        assert_eq!(saved.mapped_work_ram().len(), SUPERGRAFX_WORK_RAM_LEN);
        assert_eq!(saved.mapped_work_ram()[0x6005], 0xA5);

        write_vdc_register_on(saved.devices_mut().vdc_mut(), VdcRegister::VerticalSync, 0);
        write_vdc_register_on(
            saved.devices_mut().vdc_mut(),
            VdcRegister::VerticalDisplay,
            257,
        );
        write_vdc_register_on(
            saved.devices_mut().vdc_mut(),
            VdcRegister::VerticalDisplayEnd,
            1,
        );
        write_vdc_register_on(
            saved.devices_mut().vdc_mut(),
            VdcRegister::HorizontalDisplay,
            31,
        );
        {
            let video = saved.devices_mut().supergrafx_video_mut().unwrap();
            write_vdc_register_on(video.vdc2_mut(), VdcRegister::VerticalSync, 0);
            write_vdc_register_on(video.vdc2_mut(), VdcRegister::VerticalDisplay, 257);
            write_vdc_register_on(video.vdc2_mut(), VdcRegister::VerticalDisplayEnd, 1);
            write_vdc_register_on(video.vdc2_mut(), VdcRegister::HorizontalDisplay, 31);
            video.vpc_mut().write_port(VpcPort::from_offset(0), 0xA5);
            video.vpc_mut().write_port(VpcPort::from_offset(1), 0x20);
            video.vpc_mut().write_port(VpcPort::from_offset(2), 0x55);
            video.vpc_mut().write_port(VpcPort::from_offset(3), 1);
            video.vpc_mut().write_port(VpcPort::from_offset(4), 0xAA);
            video.vpc_mut().write_port(VpcPort::from_offset(5), 2);
            video.vpc_mut().write_port(VpcPort::from_offset(6), 1);
        }

        let frame_ticks = PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE * 262;
        saved
            .advance_devices_for_test(
                frame_ticks + 9 * PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE + 17,
            )
            .unwrap();
        assert_eq!(saved.vce_line_index(), 9);
        assert!(saved.presented_frame().active_bounds().is_some());

        saved.devices_mut().vdc_mut().vram_mut()[0x100] = 0x1111;
        write_vdc_register_on(
            saved.devices_mut().vdc_mut(),
            VdcRegister::DmaSource,
            0x0100,
        );
        write_vdc_register_on(
            saved.devices_mut().vdc_mut(),
            VdcRegister::DmaDestination,
            0x0200,
        );
        write_vdc_register_on(saved.devices_mut().vdc_mut(), VdcRegister::DmaLength, 7);
        saved.devices_mut().vdc_mut().queue_vram_dma();
        {
            let vdc2 = saved
                .devices_mut()
                .supergrafx_video_mut()
                .unwrap()
                .vdc2_mut();
            vdc2.vram_mut()[0x300] = 0x2222;
            write_vdc_register_on(vdc2, VdcRegister::DmaSource, 0x0300);
            write_vdc_register_on(vdc2, VdcRegister::DmaDestination, 0x0400);
            write_vdc_register_on(vdc2, VdcRegister::DmaLength, 11);
            vdc2.queue_vram_dma();
        }

        let bytes = encode_state(&saved).unwrap();
        let mut restored = test_supergrafx_machine(rom);
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), bytes);
        assert_eq!(restored.mapped_work_ram()[0x6005], 0xA5);
        assert_eq!(
            restored
                .devices()
                .vdc()
                .pending_vram_dma()
                .unwrap()
                .remaining_words(),
            8
        );
        let video = restored.devices().supergrafx_video().unwrap();
        assert_eq!(video.vdc2().vram()[0x300], 0x2222);
        assert_eq!(
            video.vdc2().pending_vram_dma().unwrap().remaining_words(),
            12
        );
        let vpc = video.vpc().debug_snapshot();
        assert_eq!(vpc.priority_control, [0xA5, 0x20]);
        assert_eq!(vpc.window_width, [0x155, 0x2AA]);
        assert_eq!(vpc.direct_vdc, super::super::VpcVdc::Two);

        saved.advance_devices_for_test(37_019).unwrap();
        restored.advance_devices_for_test(37_019).unwrap();
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
    }

    #[test]
    fn supergrafx_rejects_malformed_and_mismatched_topology_without_mutation() {
        let rom = test_rom();
        let mut target = test_supergrafx_machine(rom.clone());
        target.advance_devices_for_test(12_345).unwrap();
        let before = encode_state(&target).unwrap();

        let mut malformed = before.clone();
        let vpc = body_section_payload(&malformed, 4);
        *malformed.get_mut(vpc.end - 1).unwrap() = 2;
        let error = decode_state(&mut target, &malformed).unwrap_err();
        assert!(format!("{error:#}").contains("direct-VDC"));
        assert_eq!(encode_state(&target).unwrap(), before);

        let base = encode_state(&test_machine(rom)).unwrap();
        let error = decode_state(&mut target, &base).unwrap_err();
        assert!(error.to_string().contains("topology"));
        assert_eq!(encode_state(&target).unwrap(), before);
    }

    #[test]
    fn cd_roundtrips_mid_command_and_continues_read6_dma_adpcm_exactly() {
        let disc = test_cd_disc();
        let mut saved = cd_machine(disc.clone(), true);
        {
            let cd = saved.devices_mut().cdrom2_mut().unwrap();
            cd.write_physical(CDROM2_REGISTER_START + 8, 0);
            cd.write_physical(CDROM2_REGISTER_START + 9, 0);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x03);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x0C);
            cd.write_physical(CDROM2_REGISTER_START + 8, 0xFF);
            cd.write_physical(CDROM2_REGISTER_START + 9, 0xFF);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x10);
            cd.write_physical(CDROM2_REGISTER_START + 14, 0);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x60);
            select_cd(cd);
            send_cd_command(cd, &[8, 0, 0]);
        }

        let command_bytes = encode_state(&saved).unwrap();
        let mut restored = cd_machine(disc, true);
        decode_state(&mut restored, &command_bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), command_bytes);

        for machine in [&mut saved, &mut restored] {
            let cd = machine.devices_mut().cdrom2_mut().unwrap();
            cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
            send_cd_command(cd, &[0, 2, 0]);
            cd.write_physical(CDROM2_REGISTER_START + 11, 2);
        }
        let before_arrival = PROVISIONAL_CDROM2_PHASE_TICKS + 858_313;
        saved.advance_devices_for_test(before_arrival).unwrap();
        restored.advance_devices_for_test(before_arrival).unwrap();
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );

        saved.advance_devices_for_test(300_000).unwrap();
        restored.advance_devices_for_test(300_000).unwrap();
        assert_eq!(
            restored.devices().cdrom2().unwrap().phase(),
            saved.devices().cdrom2().unwrap().phase()
        );
        assert_eq!(
            restored.devices().cdrom2().unwrap().adpcm_ram(),
            saved.devices().cdrom2().unwrap().adpcm_ram()
        );
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
        let mut saved_pcm = Vec::new();
        let mut restored_pcm = Vec::new();
        saved.drain_audio_samples_into(&mut saved_pcm);
        restored.drain_audio_samples_into(&mut restored_pcm);
        assert_eq!(restored_pcm, saved_pcm);
        assert!(saved_pcm.iter().any(|&sample| sample != 0.0));
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
    }

    #[test]
    fn arcade_card_roundtrips_live_ports_and_continues_deterministically() {
        let disc = test_cd_disc();
        let mut saved = arcade_card_machine(disc.clone());
        {
            let arcade = saved.devices_mut().arcade_card_mut().unwrap();
            let io = 0x1F_FA00;
            arcade.write_physical(io + 2, 0xFE);
            arcade.write_physical(io + 3, 0xFF);
            arcade.write_physical(io + 4, 0x1F);
            arcade.write_physical(io + 7, 3);
            arcade.write_physical(io + 9, 0x13);
            arcade.write_physical(0x08_0123, 0xA5);
            arcade.ram_mut()[1] = 0xA5;
            arcade.write_physical(io + 0x12, 0x44);
            arcade.write_physical(io + 0x13, 0x33);
            arcade.write_physical(io + 0x14, 0x12);
            arcade.write_physical(io + 0x15, 0xFC);
            arcade.write_physical(io + 0x16, 0xFF);
            arcade.write_physical(io + 0x19, 0x6A);
            arcade.write_physical(io + 0x1A, 0);
            for (register, value) in [0xEF, 0xCD, 0xAB, 0x89].into_iter().enumerate() {
                arcade.write_physical(io + 0xE0 + register as u32, value);
            }
            arcade.write_physical(io + 0xE4, 0x94);
            arcade.write_physical(io + 0xE5, 0xB9);
        }

        let bytes = encode_state(&saved).unwrap();
        let mut restored = arcade_card_machine(disc);
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), bytes);

        for machine in [&mut saved, &mut restored] {
            let arcade = machine.devices_mut().arcade_card_mut().unwrap();
            assert_eq!(arcade.read_physical(0x08_1ABC), Some(0xA5));
            arcade.write_physical(0x1F_FA10, 0x7C);
            arcade.write_physical(0x1F_FAE4, 0x88);
        }
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
    }

    #[test]
    fn arcade_card_rejects_malformed_and_mismatched_topology_without_mutation() {
        let disc = test_cd_disc();
        let mut target = arcade_card_machine(disc.clone());
        target.devices_mut().arcade_card_mut().unwrap().ram_mut()[0x12345] = 0xA5;
        let before = encode_state(&target).unwrap();

        let mut mismatched = before.clone();
        mismatched[ARCADE_CARD_OFFSET] = 0;
        let error = decode_state(&mut target, &mismatched).unwrap_err();
        assert!(error.to_string().contains("Arcade Card topology"));
        assert_eq!(encode_state(&target).unwrap(), before);

        let mut malformed = before.clone();
        let arcade = body_section_payload(&malformed, 7);
        malformed[arcade.start + super::super::ARCADE_CARD_RAM_LEN + 8] = 0x80;
        let error = decode_state(&mut target, &malformed).unwrap_err();
        assert!(format!("{error:#}").contains("control"));
        assert_eq!(encode_state(&target).unwrap(), before);

        let mut short_ram = before.clone();
        let arcade = body_section_payload(&short_ram, 7);
        short_ram.remove(arcade.end - 1);
        let section_len_offset = arcade.start - size_of::<u32>();
        let section_len = u32::from_le_bytes(
            short_ram[section_len_offset..section_len_offset + 4]
                .try_into()
                .unwrap(),
        ) - 1;
        short_ram[section_len_offset..section_len_offset + 4]
            .copy_from_slice(&section_len.to_le_bytes());
        let body_len_offset = BODY_LEN_OFFSET + 32;
        let body_len = u32::from_le_bytes(
            short_ram[body_len_offset..body_len_offset + 4]
                .try_into()
                .unwrap(),
        ) - 1;
        short_ram[body_len_offset..body_len_offset + 4].copy_from_slice(&body_len.to_le_bytes());
        assert!(decode_state(&mut target, &short_ram).is_err());
        assert_eq!(encode_state(&target).unwrap(), before);

        let no_arcade = PceMachine::with_cdrom2_system_card_and_controller(
            test_system_card_rom(),
            PceHuCardBoard::SystemCardV3,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
        .unwrap();
        let error = decode_state(&mut target, &encode_state(&no_arcade).unwrap()).unwrap_err();
        assert!(error.to_string().contains("Arcade Card topology"));
        assert_eq!(encode_state(&target).unwrap(), before);
    }

    #[test]
    fn cd_roundtrips_busy_cdda_fade_source_queue_and_paused_transport() {
        let disc = test_audio_disc();
        let mut saved = cd_machine(disc.clone(), true);
        {
            let cd = saved.devices_mut().cdrom2_mut().unwrap();
            select_cd(cd);
            send_cd_command(cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        saved
            .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS)
            .unwrap();
        {
            let cd = saved.devices_mut().cdrom2_mut().unwrap();
            assert_eq!(cd.phase(), CdScsiPhase::Busy);
            assert_eq!(cd.audio_status(), CdAudioStatus::Playing);
            cd.write_physical(CDROM2_REGISTER_START + 15, 0x08);
        }
        saved
            .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS / 2)
            .unwrap();

        let bytes = encode_state(&saved).unwrap();
        let mut restored = cd_machine(disc.clone(), true);
        decode_state(&mut restored, &bytes).unwrap();
        assert!(!saved.devices().cdrom2().unwrap().command_trace().is_empty());
        assert!(
            restored
                .devices()
                .cdrom2()
                .unwrap()
                .command_trace()
                .is_empty()
        );
        assert_eq!(encode_state(&restored).unwrap(), bytes);
        saved.advance_devices_for_test(100_000).unwrap();
        restored.advance_devices_for_test(100_000).unwrap();
        let mut saved_pcm = Vec::new();
        let mut restored_pcm = Vec::new();
        saved.drain_audio_samples_into(&mut saved_pcm);
        restored.drain_audio_samples_into(&mut restored_pcm);
        assert_eq!(restored_pcm, saved_pcm);
        assert!(saved_pcm.iter().any(|&sample| sample != 0.0));
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );

        let mut paused = cd_machine(disc.clone(), false);
        {
            let cd = paused.devices_mut().cdrom2_mut().unwrap();
            select_cd(cd);
            send_cd_command(cd, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        paused
            .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS)
            .unwrap();
        assert_eq!(
            paused.devices().cdrom2().unwrap().audio_status(),
            CdAudioStatus::Paused
        );
        let paused_bytes = encode_state(&paused).unwrap();
        let mut restored_paused = cd_machine(disc, false);
        decode_state(&mut restored_paused, &paused_bytes).unwrap();
        paused.advance_devices_for_test(75_001).unwrap();
        restored_paused.advance_devices_for_test(75_001).unwrap();
        assert_eq!(
            encode_state(&restored_paused).unwrap(),
            encode_state(&paused).unwrap()
        );
    }

    #[test]
    fn cd_roundtrips_cpu_adpcm_dma_between_nibbles_and_preserves_half_end_irq() {
        let disc = test_cd_disc();
        let mut saved = cd_machine(disc.clone(), true);
        {
            let cd = saved.devices_mut().cdrom2_mut().unwrap();
            cd.write_physical(CDROM2_REGISTER_START + 8, 0xFF);
            cd.write_physical(CDROM2_REGISTER_START + 9, 0xFF);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x0F);
            cd.write_physical(CDROM2_REGISTER_START + 10, 0xA5);
            cd.write_physical(CDROM2_REGISTER_START + 10, 0x5A);
            cd.write_physical(CDROM2_REGISTER_START + 8, 2);
            cd.write_physical(CDROM2_REGISTER_START + 9, 0);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x10);
            cd.write_physical(CDROM2_REGISTER_START + 14, 0x0F);
            cd.write_physical(CDROM2_REGISTER_START + 2, 0x0C);
            cd.write_physical(CDROM2_REGISTER_START + 13, 0x60);
        }
        saved.advance_devices_for_test(777).unwrap();
        let bytes = encode_state(&saved).unwrap();
        let mut restored = cd_machine(disc, true);
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), bytes);
        assert_eq!(saved.devices().cdrom2().unwrap().adpcm_ram()[0xFFFF], 0xA5);
        assert_eq!(saved.devices().cdrom2().unwrap().adpcm_ram()[0], 0x5A);

        saved.advance_devices_for_test(566).unwrap();
        restored.advance_devices_for_test(566).unwrap();
        assert!(
            saved
                .devices()
                .cdrom2()
                .unwrap()
                .debug_snapshot()
                .adpcm_half_irq
        );
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );

        saved.advance_devices_for_test(2_013).unwrap();
        restored.advance_devices_for_test(2_013).unwrap();
        let snapshot = saved.devices().cdrom2().unwrap().debug_snapshot();
        assert!(snapshot.adpcm_end_irq);
        assert!(!snapshot.adpcm_playing);
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&saved).unwrap()
        );
        let mut saved_pcm = Vec::new();
        let mut restored_pcm = Vec::new();
        saved.drain_audio_samples_into(&mut saved_pcm);
        restored.drain_audio_samples_into(&mut restored_pcm);
        assert_eq!(restored_pcm, saved_pcm);
    }

    #[test]
    fn cd_rejects_media_system_card_board_and_malformed_state_without_mutation() {
        let system_card = test_system_card_rom();
        let saved_disc = test_cd_disc();
        let saved = PceMachine::with_cdrom2(system_card.clone(), saved_disc.clone()).unwrap();
        let bytes = encode_state(&saved).unwrap();

        let mut wrong_media =
            PceMachine::with_cdrom2(system_card.clone(), test_cd_disc_with_seed(1)).unwrap();
        let wrong_media_before = encode_state(&wrong_media).unwrap();
        assert!(
            decode_state(&mut wrong_media, &bytes)
                .unwrap_err()
                .to_string()
                .contains("CD media")
        );
        assert_eq!(encode_state(&wrong_media).unwrap(), wrong_media_before);

        let mut other_system_card = system_card.clone();
        other_system_card[0x100] ^= 0xFF;
        let mut wrong_system_card =
            PceMachine::with_cdrom2(other_system_card, saved_disc.clone()).unwrap();
        let wrong_system_card_before = encode_state(&wrong_system_card).unwrap();
        assert!(
            decode_state(&mut wrong_system_card, &bytes)
                .unwrap_err()
                .to_string()
                .contains("System Card ROM")
        );
        assert_eq!(
            encode_state(&wrong_system_card).unwrap(),
            wrong_system_card_before
        );

        let mut wrong_board = PceMachine::with_cdrom2_system_card_and_controller(
            system_card,
            PceHuCardBoard::SystemCardV3,
            saved_disc.clone(),
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
        .unwrap();
        let wrong_board_before = encode_state(&wrong_board).unwrap();
        assert!(
            decode_state(&mut wrong_board, &bytes)
                .unwrap_err()
                .to_string()
                .contains("board")
        );
        assert_eq!(encode_state(&wrong_board).unwrap(), wrong_board_before);

        let mut malformed = bytes;
        let cd_section = body_section_payload(&malformed, 6);
        let phase_offset =
            cd_section.start + CDROM2_WORK_RAM_LEN + CDROM2_BRAM_LEN + CDROM2_ADPCM_RAM_LEN + 2;
        malformed[phase_offset] = 0xFF;
        let mut target = PceMachine::with_cdrom2(test_system_card_rom(), saved_disc).unwrap();
        target.set_sample_generation_enabled(false);
        let target_before = encode_state(&target).unwrap();
        let error = decode_state(&mut target, &malformed).unwrap_err();
        assert!(format!("{error:#}").contains("SCSI-phase"), "{error:#}");
        assert_eq!(encode_state(&target).unwrap(), target_before);
    }

    #[test]
    fn cd_roundtrips_super_system_card_ram_and_board_identity() {
        let system_card = test_system_card_rom();
        let disc = test_cd_disc();
        let mut saved = PceMachine::with_cdrom2_system_card_and_controller(
            system_card.clone(),
            PceHuCardBoard::SystemCardV3,
            disc.clone(),
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
        .unwrap();
        saved.set_sample_generation_enabled(false);
        saved.cpu_mut().cpu_mut().set_mapping_register(2, 0x68);
        saved.debug_write_cpu8(0x4000, 0x3C);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = PceMachine::with_cdrom2_system_card_and_controller(
            system_card,
            PceHuCardBoard::SystemCardV3,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
        .unwrap();
        restored.set_sample_generation_enabled(false);
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(restored.debug_peek_physical8(0x0D_0000), 0x3C);
        assert_eq!(encode_state(&restored).unwrap(), bytes);
    }

    #[test]
    fn rejects_faulted_and_roundtrips_reset_cd_target() {
        let rom = test_rom();
        let valid_bytes = encode_state(&test_machine(rom.clone())).unwrap();
        let mut faulted = test_machine(rom.clone());
        assert!(faulted.force_unsupported_opcode_trap_after_fetch().is_err());
        assert!(faulted.faulted());
        assert!(
            encode_state(&faulted)
                .unwrap_err()
                .to_string()
                .contains("faulted")
        );
        decode_state(&mut faulted, &valid_bytes).unwrap();
        assert!(!faulted.faulted());

        let track = CdTrack::from_index1_data(
            1,
            4,
            None,
            0,
            CdTrackMode::Mode1_2048,
            vec![0; CD_USER_SECTOR_BYTES],
        )
        .unwrap();
        let disc = CdDisc::new(vec![track]).unwrap();
        let system_card = vec![0; super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
        let mut cd = PceMachine::with_cdrom2(system_card.clone(), disc.clone()).unwrap();
        cd.set_sample_generation_enabled(false);
        let cd_bytes = encode_state(&cd).unwrap();
        let mut restored = PceMachine::with_cdrom2(system_card, disc).unwrap();
        restored.set_sample_generation_enabled(false);
        decode_state(&mut restored, &cd_bytes).unwrap();
        assert_eq!(encode_state(&restored).unwrap(), cd_bytes);
    }
}
