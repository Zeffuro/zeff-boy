use super::cpu::{InterruptSource, IrqPort, LineLevel, StatusFlags};
use super::{
    BaseBus, CD_USER_SECTOR_BYTES, CDROM2_BRAM_START, CDROM2_REGISTER_START, CDROM2_WORK_RAM_END,
    CDROM2_WORK_RAM_START, CdAudioEndMode, CdAudioStatus, CdDisc, CdRom2, CdScsiPhase, CdTrack,
    CdTrackMode, ControllerPort, PROVISIONAL_CDROM2_AUTO_ACK_TICKS,
    PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS, PROVISIONAL_CDROM2_PHASE_TICKS,
    PROVISIONAL_CDROM2_READ_STARTUP_SECTORS, PROVISIONAL_CDROM2_SELECTION_TICKS, PceConsoleWiring,
    PceDevices, PceHuCardBoard, PceMachine, SUPER_SYSTEM_CARD_RAM_LEN, SYSTEM_CARD_V1_V2_IMAGE_LEN,
};

const READ_STARTUP_TICKS: u64 = 859_090;

fn disc() -> CdDisc {
    let mut bytes = vec![0; CD_USER_SECTOR_BYTES * 2];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, bytes).unwrap(),
    ])
    .unwrap()
}

fn system_card_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN];
    let [low, high] = 0xE000_u16.to_le_bytes();
    rom[0x1FFE] = low;
    rom[0x1FFF] = high;
    rom
}

fn select(cd: &mut CdRom2) {
    cd.write_physical(CDROM2_REGISTER_START, 0);
    assert_eq!(cd.phase(), CdScsiPhase::Selection);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_SELECTION_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Command);
}

fn send_command(cd: &mut CdRom2, command: &[u8]) {
    for (index, &byte) in command.iter().enumerate() {
        assert_ne!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
        cd.write_physical(CDROM2_REGISTER_START + 1, byte);
        cd.write_physical(CDROM2_REGISTER_START + 2, 0x80);
        cd.write_physical(CDROM2_REGISTER_START + 2, 0);
        if index + 1 != command.len() {
            cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
        }
    }
}

fn manual_read(cd: &mut CdRom2) -> u8 {
    let value = cd.read_physical(CDROM2_REGISTER_START + 1).unwrap();
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x80);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0);
    value
}

fn manual_response(cd: &mut CdRom2, len: usize) -> Vec<u8> {
    let mut response = Vec::with_capacity(len);
    for index in 0..len {
        response.push(manual_read(cd));
        if index + 1 != len {
            cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
        }
    }
    response
}

fn finish_good_status(cd: &mut CdRom2) {
    finish_status(cd, 0);
}

fn finish_status(cd: &mut CdRom2, expected: u8) {
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert_eq!(manual_read(cd), expected);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::MessageIn);
    assert_eq!(manual_read(cd), 0);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::BusFree);
}

fn request_sense(cd: &mut CdRom2, allocation_length: u8) -> Vec<u8> {
    select(cd);
    send_command(cd, &[3, 0, 0, 0, allocation_length, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    let response = manual_response(cd, 18);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(cd);
    response
}

fn audio_disc(sectors: usize) -> CdDisc {
    let mut raw = vec![0; sectors * 2_352];
    for (index, frame) in raw.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let left = (0x1000_i16).wrapping_add(index as i16);
        let right = -left;
        frame[..2].copy_from_slice(&left.to_le_bytes());
        frame[2..].copy_from_slice(&right.to_le_bytes());
    }
    CdDisc::new(vec![
        CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, raw).unwrap(),
    ])
    .unwrap()
}

fn constant_audio_disc() -> CdDisc {
    let mut raw = vec![0; 2_352];
    for frame in raw.as_chunks_mut::<4>().0 {
        frame[..2].copy_from_slice(&0x1234_i16.to_le_bytes());
        frame[2..].copy_from_slice(&(-0x1235_i16).to_le_bytes());
    }
    CdDisc::new(vec![
        CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, raw).unwrap(),
    ])
    .unwrap()
}

fn mixed_audio_disc() -> CdDisc {
    let data = CdTrack::from_index1_data(
        1,
        4,
        None,
        0,
        CdTrackMode::Mode1_2048,
        vec![0; CD_USER_SECTOR_BYTES],
    )
    .unwrap();
    let audio =
        CdTrack::from_index1_data(2, 0, Some(2), 3, CdTrackMode::Audio, vec![0x40; 2 * 2_352])
            .unwrap();
    CdDisc::new(vec![data, audio]).unwrap()
}

fn complete_audio_start(cd: &mut CdRom2, command: &[u8; 10]) {
    select(cd);
    send_command(cd, command);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Busy);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(cd);
}

fn start_audio_track_one(cd: &mut CdRom2) {
    complete_audio_start(cd, &[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
}

#[test]
fn system_card_mirrors_rom_twice_and_leaves_upper_aperture_open() {
    let mut rom = vec![0; SYSTEM_CARD_V1_V2_IMAGE_LEN];
    rom[0] = 0x12;
    rom[0x3_FFFF] = 0xA4;
    let bus = BaseBus::with_hucard(rom, PceHuCardBoard::SystemCardV1V2, ()).unwrap();

    assert_eq!(bus.peek(0), 0x12);
    assert_eq!(bus.peek(0x04_0000), 0x12);
    assert_eq!(bus.peek(0x07_FFFF), 0xA4);
    assert_eq!(bus.peek(0x08_0000), 0xFF);
}

#[test]
fn super_system_card_ram_is_unique_bounded_and_cleared_by_power_reset() {
    let mut bus = BaseBus::with_hucard(
        vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN],
        PceHuCardBoard::SystemCardV3,
        (),
    )
    .unwrap();
    bus.write(0x0D_0000, 0x68);
    bus.write(0x0F_FFFF, 0x7F);
    bus.write(0x0C_FFFF, 0x11);
    assert_eq!(bus.read(0x0D_0000), 0x68);
    assert_eq!(bus.read(0x0F_FFFF), 0x7F);
    assert_eq!(bus.read(0x0C_FFFF), 0xFF);
    bus.reset_hucard();
    assert_eq!(bus.read(0x0D_0000), 0);
    assert_eq!(bus.read(0x0F_FFFF), 0);
}

#[test]
fn cd_work_ram_is_unique_and_bram_lock_survives_reset() {
    let mut cd = CdRom2::new(disc());
    assert!(cd.write_physical(CDROM2_WORK_RAM_START, 0x21));
    assert!(cd.write_physical(CDROM2_WORK_RAM_END, 0x87));
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_START), Some(0x21));
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_END), Some(0x87));
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_END + 1), None);

    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0xFF));
    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    assert_eq!(&cd.bram()[..8], b"HUBM\x00\xA0\x10\x80");
    cd.read_physical(CDROM2_REGISTER_START + 3);
    cd.write_physical(CDROM2_BRAM_START, 0x44);
    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    cd.write_physical(CDROM2_BRAM_START, 0x55);
    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0x55));
    cd.read_physical(CDROM2_REGISTER_START + 3);
    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0xFF));
    cd.reset();
    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0x55));
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_START), Some(0));
}

#[test]
fn cdda_sample_latch_reads_little_endian_and_ignores_read_only_writes() {
    let mut cd = CdRom2::new(constant_audio_disc());
    start_audio_track_one(&mut cd);
    cd.advance_master_ticks(700);
    cd.write_physical(CDROM2_REGISTER_START + 5, 0);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 5), Some(0xCB));
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 6), Some(0xED));

    cd.advance_master_ticks(700);
    cd.write_physical(CDROM2_REGISTER_START + 5, 0);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 5), Some(0x34));
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 6), Some(0x12));

    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    cd.write_physical(CDROM2_REGISTER_START + 3, 0xFF);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 7), Some(0x80));
    cd.write_physical(CDROM2_REGISTER_START + 6, 0xFF);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 5), Some(0x34));
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 6), Some(0x12));
}

#[test]
fn machine_power_reset_clears_both_system_card_rams_and_preserves_locked_bram() {
    let mut machine = PceMachine::with_cdrom2_system_card_and_controller(
        system_card_rom(),
        PceHuCardBoard::SystemCardV3,
        disc(),
        PceConsoleWiring::PcEngine,
        ControllerPort::default(),
    )
    .unwrap();
    let card_ram = machine.system_card_ram_mut_for_test().unwrap();
    card_ram[0] = 0x12;
    card_ram[SUPER_SYSTEM_CARD_RAM_LEN - 1] = 0x34;
    let cd = machine.devices_mut().cdrom2_mut().unwrap();
    assert!(cd.write_physical(CDROM2_WORK_RAM_START, 0x56));
    assert!(cd.write_physical(CDROM2_WORK_RAM_END, 0x78));
    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    cd.write_physical(CDROM2_BRAM_START, 0x9A);

    machine.reset();

    assert_eq!(machine.system_card_ram_mut_for_test().unwrap()[0], 0);
    assert_eq!(
        machine.system_card_ram_mut_for_test().unwrap()[SUPER_SYSTEM_CARD_RAM_LEN - 1],
        0
    );
    let cd = machine.devices_mut().cdrom2_mut().unwrap();
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_START), Some(0));
    assert_eq!(cd.read_physical(CDROM2_WORK_RAM_END), Some(0));
    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0xFF));
    cd.write_physical(CDROM2_REGISTER_START + 7, 0x80);
    assert_eq!(cd.read_physical(CDROM2_BRAM_START), Some(0x9A));
}

#[test]
fn super_system_card_id_is_present_only_with_the_v3_hardware_profile() {
    let mut original = CdRom2::new(disc());
    let mut super_card = CdRom2::with_super_system_card(disc(), true);
    for (offset, expected) in [0x00, 0xAA, 0x55, 0x03].into_iter().enumerate() {
        let address = super::cdrom2::SUPER_SYSTEM_CARD_ID_START + offset as u32;
        assert_eq!(original.peek_physical(address), Some(0xFF));
        assert_eq!(original.read_physical(address), Some(0xFF));
        assert_eq!(super_card.peek_physical(address), Some(expected));
        assert_eq!(super_card.read_physical(address), Some(expected));
    }
    assert_eq!(
        super_card.read_physical(super::cdrom2::SUPER_SYSTEM_CARD_ID_END + 1),
        None
    );
}

#[test]
fn machine_system_card_board_selects_the_matching_cd_hardware_id() {
    for (board, expected) in [
        (PceHuCardBoard::SystemCardV1V2, [0xFF; 4]),
        (PceHuCardBoard::SystemCardV3, [0x00, 0xAA, 0x55, 0x03]),
    ] {
        let machine = PceMachine::with_cdrom2_system_card_and_controller(
            system_card_rom(),
            board,
            disc(),
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
        .unwrap();
        let cdrom2 = machine.devices().cdrom2().unwrap();
        for (offset, value) in expected.into_iter().enumerate() {
            assert_eq!(
                cdrom2.peek_physical(super::cdrom2::SUPER_SYSTEM_CARD_ID_START + offset as u32),
                Some(value)
            );
        }
    }
}

#[test]
fn test_unit_ready_completes_status_message_and_bus_free_with_manual_ack() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert_eq!(manual_read(&mut cd), 0);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::MessageIn);
    assert_eq!(manual_read(&mut cd), 0);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::BusFree);
}

#[test]
fn read6_data_port_auto_acknowledges_and_irq_tracks_live_enable() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[8, 0, 0, 0, 1, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::DataIn);
    assert_eq!(cd.irq2_level(), LineLevel::High);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x40);
    assert_eq!(cd.irq2_level(), LineLevel::Low);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 8), Some(0));
    assert_eq!(
        cd.read_physical(CDROM2_REGISTER_START + 2).unwrap() & 0x80,
        0x80
    );
    assert_eq!(cd.irq2_level(), LineLevel::High);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
    assert_eq!(
        cd.read_physical(CDROM2_REGISTER_START + 2).unwrap() & 0x80,
        0
    );
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 8), Some(1));
    cd.write_physical(CDROM2_REGISTER_START + 2, 0);
    assert_eq!(cd.irq2_level(), LineLevel::High);
}

#[test]
fn unsupported_command_is_check_condition_not_host_error() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[0x7F, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert_eq!(manual_read(&mut cd), 1);
}

#[test]
fn selection_timing_is_chunk_equivalent() {
    let mut whole = CdRom2::new(disc());
    let mut split = CdRom2::new(disc());
    whole.write_physical(CDROM2_REGISTER_START, 0);
    split.write_physical(CDROM2_REGISTER_START, 0);
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START).unwrap() & 0x80,
        0
    );
    whole.advance_master_ticks(PROVISIONAL_CDROM2_SELECTION_TICKS);
    split.advance_master_ticks(17);
    split.advance_master_ticks(PROVISIONAL_CDROM2_SELECTION_TICKS - 17);
    assert_eq!(whole.phase(), split.phase());
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START),
        split.read_physical(CDROM2_REGISTER_START)
    );
}

fn pending_audio_start(generation_enabled: bool) -> CdRom2 {
    let mut cd = CdRom2::new(audio_disc(2));
    cd.set_sample_rate(48_000);
    cd.set_sample_generation_enabled(generation_enabled);
    select(&mut cd);
    send_command(&mut cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd
}

fn mixed_samples(cd: &mut CdRom2, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    cd.mix_audio_samples_into(&mut output);
    output
}

#[test]
fn audio_start_and_stop_protocol_boundaries_are_chunk_equivalent() {
    let mut whole = pending_audio_start(true);
    let mut split = pending_audio_start(true);
    let after_start = 100_000;
    whole.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + after_start);
    split.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    split.advance_master_ticks(after_start);
    assert_eq!(whole.phase(), split.phase());
    assert_eq!(
        whole.audio_transport_for_test(),
        split.audio_transport_for_test()
    );
    let whole_pcm = mixed_samples(&mut whole, 256);
    let split_pcm = mixed_samples(&mut split, 256);
    assert_eq!(whole_pcm, split_pcm);
    assert!(whole_pcm.iter().any(|sample| *sample != 0.0));

    for cd in [&mut whole, &mut split] {
        while cd.phase() != CdScsiPhase::BusFree {
            if cd.phase() == CdScsiPhase::Status || cd.phase() == CdScsiPhase::MessageIn {
                manual_read(cd);
            }
            cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        }
        select(cd);
        send_command(cd, &[0xD9, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    }
    whole.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + 50_000);
    split.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    split.advance_master_ticks(50_000);
    assert_eq!(whole.phase(), split.phase());
    assert_eq!(
        whole.audio_transport_for_test(),
        split.audio_transport_for_test()
    );
    assert_eq!(
        mixed_samples(&mut whole, 128),
        mixed_samples(&mut split, 128)
    );
}

fn pending_read6_with_live_adpcm() -> CdRom2 {
    let mut cd = CdRom2::new(disc());
    cd.set_sample_rate(48_000);
    cd.write_physical(CDROM2_REGISTER_START + 8, 0);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x03);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x0C);
    cd.write_physical(CDROM2_REGISTER_START + 8, 0xFF);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0xFF);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x10);
    cd.write_physical(CDROM2_REGISTER_START + 14, 0);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x60);
    select(&mut cd);
    send_command(&mut cd, &[8, 0, 0, 0, 1, 0]);
    cd.write_physical(CDROM2_REGISTER_START + 11, 2);
    cd
}

#[test]
fn read6_arrival_and_live_adpcm_are_chunk_equivalent() {
    let mut whole = pending_read6_with_live_adpcm();
    let mut split = pending_read6_with_live_adpcm();
    let sector_ticks = READ_STARTUP_TICKS;
    let after_arrival = 100_000;
    whole.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + sector_ticks + after_arrival);
    split.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    split.advance_master_ticks(sector_ticks);
    split.advance_master_ticks(after_arrival);
    assert_eq!(whole.phase(), split.phase());
    assert_eq!(whole.adpcm_ram(), split.adpcm_ram());
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START + 12),
        split.read_physical(CDROM2_REGISTER_START + 12)
    );
    let whole_pcm = mixed_samples(&mut whole, 512);
    let split_pcm = mixed_samples(&mut split, 512);
    assert_eq!(whole_pcm, split_pcm);
    assert!(whole_pcm.iter().any(|sample| *sample != 0.0));
}

#[test]
fn coincident_auto_ack_and_sector_arrival_are_not_lost() {
    fn at_tie() -> CdRom2 {
        let mut cd = CdRom2::new(disc());
        select(&mut cd);
        send_command(&mut cd, &[8, 0, 0, 0, 2, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS);
        cd.advance_master_ticks(286_364 - PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
        assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 8), Some(0));
        cd
    }

    let mut whole = at_tie();
    let mut split = at_tie();
    whole.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
    split.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS - 1);
    split.advance_master_ticks(1);
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START),
        split.read_physical(CDROM2_REGISTER_START)
    );
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START + 2),
        split.read_physical(CDROM2_REGISTER_START + 2)
    );
    assert_eq!(whole.read_physical(CDROM2_REGISTER_START + 8), Some(1));
    assert_eq!(split.read_physical(CDROM2_REGISTER_START + 8), Some(1));
}

#[test]
fn generation_disabled_transport_crosses_protocol_boundaries_exactly() {
    let mut whole = pending_audio_start(false);
    let mut split = pending_audio_start(false);
    let after_start = 400_000;
    whole.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + after_start);
    split.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS - 1);
    split.advance_master_ticks(1);
    split.advance_master_ticks(177_777);
    split.advance_master_ticks(after_start - 177_777);
    assert_eq!(whole.phase(), split.phase());
    assert_eq!(
        whole.audio_transport_for_test(),
        split.audio_transport_for_test()
    );
    assert!(
        mixed_samples(&mut whole, 64)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(
        mixed_samples(&mut split, 64)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn machine_device_chunks_preserve_cd_protocol_audio_order() {
    fn machine_with_pending_audio_start() -> PceMachine {
        let mut machine = PceMachine::with_cdrom2(system_card_rom(), audio_disc(2)).unwrap();
        let cd = machine.devices_mut().cdrom2_mut().unwrap();
        cd.set_sample_rate(48_000);
        select(cd);
        send_command(cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        machine
    }

    let mut whole = machine_with_pending_audio_start();
    let mut split = machine_with_pending_audio_start();
    let after_start = 100_000;
    whole
        .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS + after_start)
        .unwrap();
    split
        .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS)
        .unwrap();
    split.advance_devices_for_test(after_start).unwrap();
    assert_eq!(whole.master_ticks(), split.master_ticks());
    assert_eq!(
        whole.devices().cdrom2().unwrap().audio_transport_for_test(),
        split.devices().cdrom2().unwrap().audio_transport_for_test()
    );
    let whole_pcm = mixed_samples(whole.devices_mut().cdrom2_mut().unwrap(), 256);
    let split_pcm = mixed_samples(split.devices_mut().cdrom2_mut().unwrap(), 256);
    assert_eq!(whole_pcm, split_pcm);
    assert!(whole_pcm.iter().any(|sample| *sample != 0.0));
}

#[test]
fn ack_falling_edge_advances_and_data_latch_reads_in_output_phases() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START + 1, 0x5A);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 1), Some(0x5A));
    select(&mut cd);
    cd.write_physical(CDROM2_REGISTER_START + 1, 0);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x80);
    assert_eq!(
        cd.read_physical(CDROM2_REGISTER_START + 2).unwrap() & 0x80,
        0x80
    );
    cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS * 2);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 1), Some(0));
    cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
}

#[test]
fn audio_start_uses_ten_byte_cdb_and_emits_red_book_pcm() {
    let mut raw = vec![0; 2_352];
    raw[..4].copy_from_slice(&[0x00, 0x40, 0x00, 0xC0]);
    raw[4..8].copy_from_slice(&[0x00, 0x20, 0x00, 0xE0]);
    let audio_disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, raw).unwrap(),
    ])
    .unwrap();
    let mut cd = CdRom2::new(audio_disc);
    select(&mut cd);
    send_command(&mut cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Busy);
    assert_eq!(cd.audio_status(), CdAudioStatus::Playing);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + 1_000);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    let mut mixed = [0.0; 4];
    cd.mix_audio_samples_into(&mut mixed);
    assert!(mixed[0] > 0.0);
    assert!(mixed[1] < 0.0);
}

#[test]
fn lemmings_paused_seek_past_leadout_completes_with_exact_position() {
    let mut cd = CdRom2::new(audio_disc(1));
    complete_audio_start(&mut cd, &[0xD8, 0, 0x47, 0x59, 0x24, 0, 0, 0, 0, 0x40]);
    assert_eq!(cd.audio_status(), CdAudioStatus::Paused);
    assert_eq!(cd.audio_transport_for_test(), (215_799, 1, 215_799, 0, 0));

    select(&mut cd);
    send_command(&mut cd, &[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(
        manual_response(&mut cd, 10),
        [
            CdAudioStatus::Paused as u8,
            0x01,
            0,
            0x01,
            0,
            0,
            0,
            0x47,
            0x59,
            0x24
        ]
    );
}

#[test]
fn audio_start_accepts_data_gap_audio_leadout_and_past_positions() {
    for lba in [0_u32, 2, 3, 5, 9] {
        let mut cd = CdRom2::new(mixed_audio_disc());
        let [_, high, middle, low] = lba.to_be_bytes();
        complete_audio_start(&mut cd, &[0xD8, 0, 0, high, middle, low, 0, 0, 0, 0]);
        assert_eq!(cd.audio_status(), CdAudioStatus::Paused);
        assert_eq!(cd.audio_transport_for_test(), (lba, 5, lba, 0, 0));
    }

    let mut cd = CdRom2::new(mixed_audio_disc());
    complete_audio_start(&mut cd, &[0xD8, 3, 0, 0, 0, 3, 0, 0, 0, 0]);
    assert_eq!(cd.audio_status(), CdAudioStatus::Playing);
}

#[test]
fn audio_start_address_formats_and_invalid_addresses_are_exact() {
    let mut cd = CdRom2::new(mixed_audio_disc());
    complete_audio_start(&mut cd, &[0xD8, 0, 0, 0, 0, 2, 0, 0, 0, 0]);
    assert_eq!(cd.audio_transport_for_test().0, 2);

    complete_audio_start(&mut cd, &[0xD8, 0, 0, 0x02, 0, 0, 0, 0, 0, 0x40]);
    assert_eq!(cd.audio_transport_for_test().0, 0);

    complete_audio_start(&mut cd, &[0xD8, 0, 0x02, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(cd.audio_transport_for_test().0, 3);

    complete_audio_start(&mut cd, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0xC0]);
    assert_eq!(cd.audio_transport_for_test().0, 3);

    for command in [
        [0xD8, 0, 0xFA, 0, 0, 0, 0, 0, 0, 0x40],
        [0xD8, 0, 0, 0x60, 0, 0, 0, 0, 0, 0x40],
        [0xD8, 0, 0, 0, 0x75, 0, 0, 0, 0, 0x40],
        [0xD8, 0, 0, 0, 0x01, 0, 0, 0, 0, 0x40],
        [0xD8, 0, 0x03, 0, 0, 0, 0, 0, 0, 0x80],
    ] {
        select(&mut cd);
        send_command(&mut cd, &command);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        finish_status(&mut cd, 1);
        let sense = request_sense(&mut cd, 1);
        assert_eq!((sense[2], sense[12]), (0x05, 0x21));
    }
}

#[test]
fn audio_end_modes_and_current_leadout_address_are_exact() {
    for (mode, phase, status, end, behavior) in [
        (0, CdScsiPhase::Status, CdAudioStatus::Stopped, 4, 0),
        (1, CdScsiPhase::Busy, CdAudioStatus::Playing, 1, 1),
        (2, CdScsiPhase::Busy, CdAudioStatus::Playing, 1, 2),
        (3, CdScsiPhase::Status, CdAudioStatus::Playing, 1, 0),
        (4, CdScsiPhase::Status, CdAudioStatus::Paused, 1, 0),
    ] {
        let mut cd = CdRom2::new(audio_disc(4));
        complete_audio_start(&mut cd, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        select(&mut cd);
        send_command(&mut cd, &[0xD9, mode, 0, 0, 0, 1, 0, 0, 0, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        assert_eq!(cd.phase(), phase);
        assert_eq!(cd.audio_status(), status);
        assert_eq!(cd.audio_transport_for_test().1, end);
        assert_eq!(cd.audio_transport_for_test().4, behavior);
        let snapshot = cd.audio_debug_snapshot();
        assert_eq!(snapshot.status, status);
        assert_eq!(snapshot.end_lba, end);
        assert_eq!(
            snapshot.end_mode,
            match behavior {
                0 => CdAudioEndMode::Stop,
                1 => CdAudioEndMode::Loop,
                2 => CdAudioEndMode::SignalCompletion,
                _ => unreachable!(),
            }
        );
    }

    let mut cd = CdRom2::new(audio_disc(4));
    complete_audio_start(&mut cd, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    select(&mut cd);
    send_command(&mut cd, &[0xD9, 4, 0, 0, 0, 0, 0, 0, 0, 0xC0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.audio_transport_for_test().1, 4);
    finish_good_status(&mut cd);

    select(&mut cd);
    send_command(&mut cd, &[0xD9, 5, 0, 0, 0, 1, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_status(&mut cd, 1);
    let sense = request_sense(&mut cd, 0xFF);
    assert_eq!((sense[2], sense[12]), (0x05, 0x22));
}

#[test]
fn selection_interrupts_busy_cdda_so_a_read_command_can_take_over() {
    let mut cd = CdRom2::new(mixed_audio_disc());
    complete_audio_start(&mut cd, &[0xD8, 1, 0x02, 0, 0, 0, 0, 0, 0, 0x80]);

    select(&mut cd);
    send_command(&mut cd, &[0xD9, 2, 0, 0, 0, 5, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Busy);
    assert_eq!(cd.audio_status(), CdAudioStatus::Playing);

    select(&mut cd);
    send_command(&mut cd, &[0x08, 0, 0, 0, 1, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);

    assert_eq!(cd.phase(), CdScsiPhase::DataIn);
    assert_eq!(cd.audio_status(), CdAudioStatus::Inactive);
}

#[test]
fn audio_end_mode_four_preserves_completion_behavior() {
    let mut cd = CdRom2::new(audio_disc(4));
    complete_audio_start(&mut cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    select(&mut cd);
    send_command(&mut cd, &[0xD9, 2, 0, 0, 0, 1, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    cd.advance_master_ticks(286_364);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    finish_good_status(&mut cd);
    assert_eq!(cd.audio_transport_for_test().4, 2);

    select(&mut cd);
    send_command(&mut cd, &[0xD9, 4, 0, 0, 0, 2, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(&mut cd);
    assert_eq!(cd.audio_transport_for_test().1, 2);
    assert_eq!(cd.audio_transport_for_test().4, 2);
}

#[test]
fn audio_pause_rejects_inactive_and_stopped_but_is_idempotent_when_paused() {
    let pause = [0xDA, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut inactive = CdRom2::new(audio_disc(2));
    select(&mut inactive);
    send_command(&mut inactive, &pause);
    inactive.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_status(&mut inactive, 1);
    let sense = request_sense(&mut inactive, 10);
    assert_eq!((sense[2], sense[12]), (0x05, 0x2C));

    let mut stopped = CdRom2::new(audio_disc(2));
    complete_audio_start(&mut stopped, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    select(&mut stopped);
    send_command(&mut stopped, &[0xD9, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    stopped.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(&mut stopped);
    select(&mut stopped);
    send_command(&mut stopped, &pause);
    stopped.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_status(&mut stopped, 1);
    let sense = request_sense(&mut stopped, 10);
    assert_eq!((sense[2], sense[12]), (0x05, 0x2C));

    let mut paused = CdRom2::new(audio_disc(2));
    complete_audio_start(&mut paused, &[0xD8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    select(&mut paused);
    send_command(&mut paused, &pause);
    paused.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(&mut paused);
    assert_eq!(paused.audio_status(), CdAudioStatus::Paused);

    let mut playing = CdRom2::new(audio_disc(2));
    complete_audio_start(&mut playing, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    select(&mut playing);
    send_command(&mut playing, &pause);
    playing.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_good_status(&mut playing);
    assert_eq!(playing.audio_status(), CdAudioStatus::Paused);
}

#[test]
fn audio_status_bytes_match_the_cd_subcode_protocol() {
    assert_eq!(CdAudioStatus::Playing as u8, 0);
    assert_eq!(CdAudioStatus::Inactive as u8, 1);
    assert_eq!(CdAudioStatus::Paused as u8, 2);
    assert_eq!(CdAudioStatus::Stopped as u8, 3);
}

#[test]
fn audio_pause_and_subq_report_track_and_absolute_position() {
    let mut cd = CdRom2::new(audio_disc(2));
    start_audio_track_one(&mut cd);

    select(&mut cd);
    send_command(&mut cd, &[0xDA, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.audio_status(), CdAudioStatus::Paused);
    finish_good_status(&mut cd);

    select(&mut cd);
    send_command(&mut cd, &[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(
        manual_response(&mut cd, 10),
        [
            CdAudioStatus::Paused as u8,
            0x01,
            0x01,
            0x01,
            0,
            0,
            0,
            0,
            2,
            0
        ]
    );
}

#[test]
fn subq_reports_upcoming_track_and_index_zero_countdown() {
    let first =
        CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, vec![0; 3 * 2_352]).unwrap();
    let second =
        CdTrack::from_stored_data(2, 0, Some(3), 5, CdTrackMode::Audio, vec![0; 3 * 2_352])
            .unwrap();
    let pregap_disc = CdDisc::new(vec![first, second]).unwrap();

    for (lba, index, relative) in [(3_u32, 0, 1), (4, 0, 0), (5, 1, 0)] {
        let mut cd = CdRom2::new(pregap_disc.clone());
        let [_, high, middle, low] = lba.to_be_bytes();
        complete_audio_start(&mut cd, &[0xD8, 0, 0, high, middle, low, 0, 0, 0, 0]);
        select(&mut cd);
        send_command(&mut cd, &[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        assert_eq!(
            manual_response(&mut cd, 10),
            [
                CdAudioStatus::Paused as u8,
                0x01,
                0x02,
                index,
                0,
                0,
                relative,
                0,
                2,
                lba as u8
            ]
        );
    }
}

#[test]
fn subq_uses_declared_index_zero_when_pregap_payload_was_omitted() {
    let track =
        CdTrack::from_index1_data(2, 5, Some(8), 10, CdTrackMode::Audio, vec![0; 2_352]).unwrap();
    let disc = CdDisc::new(vec![track]).unwrap();
    assert_eq!(
        disc.read_audio_sample(8, 0),
        Err(super::CdReadError::LbaOutOfRange(8))
    );
    let mut cd = CdRom2::new(disc);
    complete_audio_start(&mut cd, &[0xD8, 0, 0, 0, 0, 8, 0, 0, 0, 0]);
    select(&mut cd);
    send_command(&mut cd, &[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(
        manual_response(&mut cd, 10),
        [CdAudioStatus::Paused as u8, 0x51, 0x02, 0, 0, 0, 1, 0, 2, 8]
    );
}

#[test]
fn audio_end_completion_waits_for_the_requested_lba_and_raises_status_irq() {
    let mut cd = CdRom2::new(audio_disc(2));
    start_audio_track_one(&mut cd);

    select(&mut cd);
    send_command(&mut cd, &[0xD9, 2, 0, 0, 0, 1, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Busy);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x20);
    cd.advance_master_ticks(286_364);
    assert_eq!(cd.audio_status(), CdAudioStatus::Stopped);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert_eq!(cd.irq2_level(), LineLevel::Low);
}

#[test]
fn stopped_audio_does_not_accumulate_or_repeat_pcm() {
    let mut cd = CdRom2::new(audio_disc(1));
    start_audio_track_one(&mut cd);
    cd.advance_master_ticks(286_364);
    assert_eq!(cd.audio_status(), CdAudioStatus::Stopped);
    cd.advance_master_ticks(10_000_000);
    let mut mixed = [0.0; 64];
    cd.mix_audio_samples_into(&mut mixed);
    assert!(mixed.iter().all(|sample| *sample == 0.0));
}

#[test]
fn directory_info_modes_and_track_zero_alias_return_exact_four_byte_records() {
    let info_disc = CdDisc::new(vec![
        CdTrack::from_index1_data(
            1,
            4,
            None,
            0x01_2345,
            CdTrackMode::Mode1_2048,
            vec![0; CD_USER_SECTOR_BYTES],
        )
        .unwrap(),
    ])
    .unwrap();
    let expected = [
        [0x01, 0x01, 0, 0],
        [0x16, 0x36, 0x16, 0],
        [0x16, 0x36, 0x15, 4],
        [0x01, 0x23, 0x45, 4],
    ];
    for (mode, expected) in expected.into_iter().enumerate() {
        let mut cd = CdRom2::new(info_disc.clone());
        select(&mut cd);
        send_command(&mut cd, &[0xDE, mode as u8, 0x01, 0, 0, 0, 0, 0, 0, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        assert_eq!(manual_response(&mut cd, 4), expected);
    }

    for mode in [2, 3] {
        let mut cd = CdRom2::new(info_disc.clone());
        select(&mut cd);
        send_command(&mut cd, &[0xDE, mode, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
        assert_eq!(manual_response(&mut cd, 4), expected[mode as usize]);
    }
}

#[test]
fn request_sense_returns_fixed_record_and_clears_reported_sense() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[0x7F, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    finish_status(&mut cd, 1);
    assert_eq!(
        request_sense(&mut cd, 4),
        [
            0x70, 0, 0x05, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0x20, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(
        request_sense(&mut cd, 0),
        [0x70, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn read6_count_two_inserts_rational_sector_cadence() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[8, 0, 0, 0, 2, 0]);
    assert_eq!(PROVISIONAL_CDROM2_READ_STARTUP_SECTORS, 3);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS - 1);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    cd.advance_master_ticks(1);
    for _ in 0..CD_USER_SECTOR_BYTES {
        cd.read_physical(CDROM2_REGISTER_START + 8);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
    }
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    cd.advance_master_ticks(243_355);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    cd.advance_master_ticks(1);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
}

#[test]
fn second_sector_ready_event_is_chunk_equivalent() {
    fn at_second_sector_delay() -> CdRom2 {
        let mut cd = CdRom2::new(disc());
        select(&mut cd);
        send_command(&mut cd, &[8, 0, 0, 0, 2, 0]);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS);
        for _ in 0..CD_USER_SECTOR_BYTES {
            cd.read_physical(CDROM2_REGISTER_START + 8);
            cd.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
        }
        cd
    }

    let mut whole = at_second_sector_delay();
    let mut split = at_second_sector_delay();
    whole.advance_master_ticks(243_356);
    split.advance_master_ticks(123_456);
    split.advance_master_ticks(119_900);
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START),
        split.read_physical(CDROM2_REGISTER_START)
    );
    assert_eq!(
        whole.read_physical(CDROM2_REGISTER_START + 8),
        split.read_physical(CDROM2_REGISTER_START + 8)
    );
}

#[test]
fn already_arrived_second_sector_has_no_per_byte_request_delay() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[8, 0, 0, 0, 2, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS + 286_364);
    for _ in 0..CD_USER_SECTOR_BYTES {
        cd.read_physical(CDROM2_REGISTER_START + 8);
        cd.advance_master_ticks(PROVISIONAL_CDROM2_AUTO_ACK_TICKS);
        assert_ne!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    }
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 8), Some(0));
}

#[test]
fn cd_to_adpcm_dma_consumes_arriving_sectors_and_wraps_its_write_pointer() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START + 8, 0x00);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0xFC);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x03);
    select(&mut cd);
    send_command(&mut cd, &[8, 0, 0, 0, 2, 0]);
    cd.write_physical(CDROM2_REGISTER_START + 11, 2);

    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::DataIn);
    assert!(!cd.take_debug_dma_completed());
    assert_eq!(&cd.adpcm_ram()[0xFC00..0xFC04], &[0, 1, 2, 3]);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);

    cd.advance_master_ticks(286_364);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert!(cd.take_debug_dma_completed());
    assert!(!cd.take_debug_dma_completed());
    assert_eq!(&cd.adpcm_ram()[0x0400..0x0404], &[0, 1, 2, 3]);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 11), Some(2));
}

#[test]
fn adpcm_cpu_ports_keep_independent_wrapping_read_and_write_pointers() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START + 8, 0xFF);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0xFF);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x0F);
    cd.write_physical(CDROM2_REGISTER_START + 10, 0xA5);
    cd.write_physical(CDROM2_REGISTER_START + 10, 0x5A);
    assert_eq!(cd.adpcm_ram()[0xFFFF], 0xA5);
    assert_eq!(cd.adpcm_ram()[0], 0x5A);

    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 10), Some(0));
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 10), Some(0xA5));
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 10), Some(0x5A));
}

#[test]
fn adpcm_playback_length_drives_half_end_status_and_live_irq2() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START + 8, 2);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x10);
    cd.write_physical(CDROM2_REGISTER_START + 14, 0x0F);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x0C);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x60);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START + 12).unwrap() & 8, 0);

    cd.advance_master_ticks(1_343);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START + 3).unwrap() & 4, 0);
    assert_eq!(cd.irq2_level(), LineLevel::Low);
    cd.advance_master_ticks(1_342);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START + 12).unwrap() & 8, 0);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START + 3).unwrap() & 8, 0);
    cd.advance_master_ticks(671);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 12).unwrap() & 8, 0);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START + 3).unwrap() & 8, 0);
    assert_eq!(cd.irq2_level(), LineLevel::Low);

    cd.write_physical(CDROM2_REGISTER_START + 13, 0x10);
    assert_eq!(
        cd.read_physical(CDROM2_REGISTER_START + 3).unwrap() & 0x0C,
        0
    );
    assert_eq!(cd.irq2_level(), LineLevel::High);
}

#[test]
fn adpcm_debug_snapshot_reports_playback_position_and_remaining_length() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START + 8, 2);
    cd.write_physical(CDROM2_REGISTER_START + 9, 0);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x1C);
    cd.write_physical(CDROM2_REGISTER_START + 13, 0x2C);

    let started = cd.adpcm_debug_snapshot();
    assert_eq!(started.address_latch, 2);
    assert_eq!(started.read_address, 2);
    assert_eq!(started.write_address, 0);
    assert_eq!(started.remaining_length, 2);
    assert_eq!(started.playback_rate, 0x0F);
    assert!(started.playing);

    cd.advance_master_ticks(1_343);
    let progressed = cd.adpcm_debug_snapshot();
    assert_eq!(progressed.read_address, 3);
    assert_eq!(progressed.remaining_length, 1);
    assert!(progressed.playing);
}

#[test]
fn request_sense_zero_allocation_still_returns_fixed_record() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[3, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::DataIn);
    assert_eq!(
        manual_response(&mut cd, 18),
        [0x70, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn invalid_directory_track_bcd_returns_check_condition() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    send_command(&mut cd, &[0xDE, 2, 0xFA, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Status);
    assert_eq!(manual_read(&mut cd), 1);
}

#[test]
fn reset_register_reports_asserted_state_and_clears_live_phase() {
    let mut cd = CdRom2::new(disc());
    select(&mut cd);
    cd.write_physical(CDROM2_REGISTER_START + 4, 2);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 4), Some(2));
    assert_eq!(cd.phase(), CdScsiPhase::BusFree);
    cd.write_physical(CDROM2_REGISTER_START, 0);
    assert_eq!(cd.phase(), CdScsiPhase::BusFree);
    cd.write_physical(CDROM2_REGISTER_START + 4, 0);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 4), Some(0));
}

#[test]
fn ack_high_before_request_blocks_request_until_falling_edge() {
    let mut cd = CdRom2::new(disc());
    cd.write_physical(CDROM2_REGISTER_START, 0);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0x80);
    assert_eq!(
        cd.read_physical(CDROM2_REGISTER_START + 2).unwrap() & 0x80,
        0x80
    );
    cd.advance_master_ticks(PROVISIONAL_CDROM2_SELECTION_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Command);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    cd.write_physical(CDROM2_REGISTER_START + 2, 0);
    assert_ne!(cd.read_physical(CDROM2_REGISTER_START).unwrap() & 0x40, 0);
    assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 1), Some(0));
}

#[test]
fn base_bus_routes_exact_expansion_ranges_and_cd_controller_wiring() {
    let devices = PceDevices::with_cdrom2(
        ControllerPort::default(),
        PceConsoleWiring::PcEngine,
        disc(),
    );
    let mut bus = BaseBus::with_hucard(
        vec![0; SYSTEM_CARD_V1_V2_IMAGE_LEN],
        PceHuCardBoard::SystemCardV1V2,
        devices,
    )
    .unwrap();
    bus.write(CDROM2_WORK_RAM_START, 0x9A);
    assert_eq!(bus.read(CDROM2_WORK_RAM_START), 0x9A);
    assert_eq!(bus.read(CDROM2_REGISTER_START + 0x10), 0xFF);
    assert_eq!(bus.read(0x1F_F000) & 0xF0, 0x70);
}

#[test]
fn machine_boots_system_card_and_advances_cd_on_its_master_clock() {
    let mut machine = PceMachine::with_cdrom2(system_card_rom(), disc()).unwrap();
    assert_eq!(machine.cpu().cpu().registers().pc, 0xE000);
    machine
        .devices_mut()
        .cdrom2_mut()
        .unwrap()
        .write_physical(CDROM2_REGISTER_START, 0);
    machine
        .advance_devices_for_test(PROVISIONAL_CDROM2_SELECTION_TICKS)
        .unwrap();
    assert_eq!(machine.master_ticks(), PROVISIONAL_CDROM2_SELECTION_TICKS);
    assert_eq!(
        machine.devices().cdrom2().unwrap().phase(),
        CdScsiPhase::Command
    );
}

#[test]
fn machine_refreshes_live_cd_irq2_at_boundary() {
    let mut machine = PceMachine::with_cdrom2(system_card_rom(), disc()).unwrap();
    {
        let cd = machine.devices_mut().cdrom2_mut().unwrap();
        select(cd);
        send_command(cd, &[8, 0, 0, 0, 1, 0]);
    }
    machine
        .advance_devices_for_test(PROVISIONAL_CDROM2_PHASE_TICKS + READ_STARTUP_TICKS)
        .unwrap();
    machine
        .devices_mut()
        .cdrom2_mut()
        .unwrap()
        .write_physical(CDROM2_REGISTER_START + 2, 0x40);
    machine.advance_devices_for_test(1).unwrap();
    machine
        .cpu_mut()
        .on_chip_io_mut()
        .write_irq(IrqPort::Disable, 0);
    machine
        .cpu_mut()
        .cpu_mut()
        .registers_mut()
        .status
        .remove(StatusFlags::INTERRUPT);
    assert_eq!(
        machine.cpu().highest_priority_unmasked_request(),
        Some(InterruptSource::Irq2)
    );
}
