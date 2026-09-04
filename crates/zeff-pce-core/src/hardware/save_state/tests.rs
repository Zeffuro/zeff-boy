use super::*;
use crate::hardware::cpu::VdcPort;
use crate::hardware::{
    CD_USER_SECTOR_BYTES, CDROM2_ADPCM_RAM_LEN, CDROM2_BRAM_LEN, CDROM2_REGISTER_START,
    CDROM2_WORK_RAM_LEN, CdAudioStatus, CdDisc, CdScsiPhase, CdTrack, CdTrackMode,
    ControllerDevice, ControllerPort, HuC6270, OPEN_BUS_VALUE,
    PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS, PROVISIONAL_CDROM2_PHASE_TICKS,
    PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE, PceCartridgeDescriptor, PceCartridgeHardware,
    PceMouse, PsgPort, SUPERGRAFX_WORK_RAM_LEN, VcePort, VdcRegister, VpcPort,
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

fn io_latch_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..12].copy_from_slice(&[
        0xA9, 0xFF, 0x53, 0x01, 0xA9, 0x20, 0x8D, 0x00, 0x08, 0x4C, 0x09, 0xE0,
    ]);
    rom[0x1FFE] = 0;
    rom[0x1FFF] = 0xE0;
    rom
}

fn test_machine(rom: Vec<u8>) -> PceMachine {
    let mut machine = PceMachine::new(rom).unwrap();
    machine.set_sample_generation_enabled(false);
    machine
}

fn test_supergrafx_machine(rom: Vec<u8>) -> PceMachine {
    let descriptor =
        PceCartridgeDescriptor::default().with_required_hardware(PceCartridgeHardware::SuperGrafx);
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

fn remove_body_section_byte(bytes: &mut Vec<u8>, section_index: usize, offset: usize) {
    let section = body_section_payload(bytes, section_index);
    let section_len_offset = section.start - size_of::<u32>();
    let section_len =
        u32::from_le_bytes(bytes[section_len_offset..section.start].try_into().unwrap());
    let body_len_offset = BODY_LEN_OFFSET
        + if bytes[CONTENT_OFFSET] == CONTENT_CD {
            32
        } else {
            0
        };
    let body_len = u32::from_le_bytes(
        bytes[body_len_offset..body_len_offset + size_of::<u32>()]
            .try_into()
            .unwrap(),
    );

    bytes.remove(offset);
    bytes[section_len_offset..section.start].copy_from_slice(&(section_len - 1).to_le_bytes());
    bytes[body_len_offset..body_len_offset + size_of::<u32>()]
        .copy_from_slice(&(body_len - 1).to_le_bytes());
}

fn downgrade_to_v1(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes = downgrade_to_v2(bytes);
    let cpu = body_section_payload(&bytes, 0);
    remove_body_section_byte(&mut bytes, 0, cpu.end - 3);
    bytes[VERSION_OFFSET..VERSION_OFFSET + 4].copy_from_slice(&LEGACY_VERSION.to_le_bytes());
    bytes
}

fn downgrade_to_v2(mut bytes: Vec<u8>) -> Vec<u8> {
    for _ in 0..size_of::<u64>() {
        let timing = body_section_payload(&bytes, 6);
        remove_body_section_byte(&mut bytes, 6, timing.end - 1);
    }
    bytes[VERSION_OFFSET..VERSION_OFFSET + 4].copy_from_slice(&PREVIOUS_VERSION.to_le_bytes());
    bytes
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
    let track = CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, bytes).unwrap();
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
fn current_state_roundtrips_and_legacy_versions_load_defaults() {
    let rom = io_latch_rom();
    let mut saved = test_machine(rom.clone());
    run_boundaries(&mut saved, 4);
    let current = encode_state(&saved).unwrap();
    let cpu = body_section_payload(&current, 0);
    assert_eq!(
        u32::from_le_bytes(current[VERSION_OFFSET..12].try_into().unwrap()),
        PCE_SAVE_STATE_FORMAT_VERSION
    );
    assert_eq!(current[cpu.end - 3], 0x20);

    let mut restored_current = test_machine(rom.clone());
    decode_state(&mut restored_current, &current).unwrap();
    assert_eq!(encode_state(&restored_current).unwrap(), current);

    let v2 = downgrade_to_v2(current.clone());
    let mut restored_v2 = test_machine(rom.clone());
    decode_state(&mut restored_v2, &v2).unwrap();
    assert_eq!(restored_v2.frame_count(), 0);

    let v1 = downgrade_to_v1(current);
    let mut restored_v1 = test_machine(rom);
    decode_state(&mut restored_v1, &v1).unwrap();
    let migrated_v1 = encode_state(&restored_v1).unwrap();
    let cpu = body_section_payload(&migrated_v1, 0);
    assert_eq!(migrated_v1[cpu.end - 3], OPEN_BUS_VALUE);
}

#[test]
fn v2_load_defaults_the_unstored_frame_count() {
    let rom = test_rom();
    let mut saved = test_machine(rom.clone());
    assert_eq!(saved.run_until_frame().unwrap().frames_published(), 1);
    assert_eq!(saved.frame_count(), 1);
    let v2 = downgrade_to_v2(encode_state(&saved).unwrap());

    let mut restored = test_machine(rom);
    decode_state(&mut restored, &v2).unwrap();

    assert_eq!(restored.frame_count(), 0);
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
    wrong_version[VERSION_OFFSET..VERSION_OFFSET + 4]
        .copy_from_slice(&(PCE_SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());
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
    trailing[BODY_LEN_OFFSET..BODY_LEN_OFFSET + 4].copy_from_slice(&(body_len + 1).to_le_bytes());
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
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Populous);
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
    let mut system_card = PceMachine::with_cartridge(system_card_rom.clone(), descriptor).unwrap();
    system_card.set_sample_generation_enabled(false);
    run_boundaries(&mut system_card, 4);
    assert_eq!(system_card.debug_peek_physical8(0x0D_0000), 0x3C);
    let system_card_bytes = encode_state(&system_card).unwrap();
    let mut restored_system_card = PceMachine::with_cartridge(system_card_rom, descriptor).unwrap();
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
        .advance_devices_for_test(frame_ticks + 9 * PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE + 17)
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

#[path = "tests/cd.rs"]
mod cd;
