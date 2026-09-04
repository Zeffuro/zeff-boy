use super::*;

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
    malformed[arcade.start + super::super::super::ARCADE_CARD_RAM_LEN + 8] = 0x80;
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
    let system_card = vec![0; super::super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    let mut cd = PceMachine::with_cdrom2(system_card.clone(), disc.clone()).unwrap();
    cd.set_sample_generation_enabled(false);
    let cd_bytes = encode_state(&cd).unwrap();
    let mut restored = PceMachine::with_cdrom2(system_card, disc).unwrap();
    restored.set_sample_generation_enabled(false);
    decode_state(&mut restored, &cd_bytes).unwrap();
    assert_eq!(encode_state(&restored).unwrap(), cd_bytes);
}
