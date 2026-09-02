use super::*;
use crate::hardware::{
    CD_USER_SECTOR_BYTES, CdDisc, CdTrack, CdTrackMode, MEMORY_BASE128_RAM_LEN,
    PceCartridgeHardware, SYSTEM_CARD_V1_V2_IMAGE_LEN, SixButtonExtraButtons,
};

fn rom(seed: u8) -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[0] = seed;
    rom[0x1FFE] = 0;
    rom[0x1FFF] = 0;
    rom
}

fn machine(rom: Vec<u8>) -> PceMachine {
    let mut machine = PceMachine::with_controller(rom, ControllerPort::two_button()).unwrap();
    machine.set_sample_generation_enabled(false);
    machine
}

fn pce_cd_machine(system_card_seed: u8, disc_seed: u8) -> PceMachine {
    pce_cd_machine_with_arcade_card(system_card_seed, disc_seed, false)
}

fn pce_cd_machine_with_arcade_card(
    system_card_seed: u8,
    disc_seed: u8,
    arcade_card_enabled: bool,
) -> PceMachine {
    let mut system_card = vec![0xEA; SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[0] = system_card_seed;
    system_card[0x1FFE] = 0;
    system_card[0x1FFF] = 0;
    let mut sector = vec![disc_seed; CD_USER_SECTOR_BYTES];
    sector[0] ^= 0xFF;
    let track = CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, sector).unwrap();
    let mut machine = PceMachine::with_cdrom2_system_card_controller_and_arcade_card(
        system_card,
        PceHuCardBoard::SystemCardV3,
        CdDisc::new(vec![track]).unwrap(),
        PceConsoleWiring::PcEngine,
        ControllerPort::two_button(),
        arcade_card_enabled,
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine.set_sample_generation_enabled(false);
    machine
}

fn advance_frames(machine: &mut PceMachine, frames: u64) {
    for _ in 0..frames {
        assert_eq!(machine.run_until_frame().unwrap().frames_published(), 1);
    }
}

#[test]
fn current_state_restores_exact_identity_output_and_continuation() {
    let rom = rom(0xEA);
    let mut source = machine(rom.clone());
    source
        .devices_mut()
        .controller_mut()
        .two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::I | PadButtons::RIGHT);
    advance_frames(&mut source, 3);
    let state = super::super::encode_state(&source).unwrap();
    let mut restored = machine(rom);

    let inspection = inspect_current_native_direct_hucard_tas_state(&restored, &state).unwrap();
    assert_eq!(inspection.normalized_rom_len, source.hucard_rom().len());
    assert_eq!(inspection.board, PceHuCardBoard::Plain);
    assert_eq!(inspection.topology, PceHardwareTopology::Base);
    assert_eq!(inspection.psg_revision, PsgRevision::HuC6280);
    assert_eq!(
        inspection.controller_buttons,
        PadButtons::I | PadButtons::RIGHT
    );
    assert_eq!(inspection.projection.frame_count, 3);
    assert_eq!(
        inspection.projection.framebuffer.as_ref(),
        source.framebuffer()
    );

    let projection =
        validate_and_load_current_native_direct_hucard_tas_state(&mut restored, &state).unwrap();
    assert_eq!(projection, inspection.projection);
    assert_eq!(super::super::encode_state(&restored).unwrap(), state);

    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(source.framebuffer(), restored.framebuffer());
    assert_eq!(
        super::super::encode_state(&source).unwrap(),
        super::super::encode_state(&restored).unwrap()
    );
}

#[test]
fn pce_cd_state_restores_exact_identity_output_and_continuation() {
    let mut source = pce_cd_machine(0x42, 0x15);
    source
        .devices_mut()
        .controller_mut()
        .two_button_pad_mut()
        .unwrap()
        .set_buttons(PadButtons::II | PadButtons::LEFT);
    advance_frames(&mut source, 3);
    let state = super::super::encode_state(&source).unwrap();
    let mut restored = pce_cd_machine(0x42, 0x15);

    let inspection = inspect_current_native_pce_cd_tas_state(&restored, &state).unwrap();
    assert_eq!(inspection.system_card_len, SYSTEM_CARD_V1_V2_IMAGE_LEN);
    assert_eq!(inspection.board, PceHuCardBoard::SystemCardV3);
    assert_eq!(inspection.wiring, PceConsoleWiring::PcEngine);
    assert_eq!(inspection.psg_revision, PsgRevision::HuC6280);
    assert_eq!(
        inspection.controller_buttons,
        PadButtons::II | PadButtons::LEFT
    );
    assert_eq!(inspection.projection.frame_count, 3);
    assert_eq!(
        inspection.projection.framebuffer.as_ref(),
        source.framebuffer()
    );
    let identity = inspect_current_native_pce_cd_tas_state_identity(&state).unwrap();
    assert_eq!(identity.system_card_sha256, inspection.system_card_sha256);
    assert_eq!(identity.disc_sha256, inspection.disc_sha256);

    let projection =
        validate_and_load_current_native_pce_cd_tas_state(&mut restored, &state).unwrap();
    assert_eq!(projection, inspection.projection);
    assert_eq!(super::super::encode_state(&restored).unwrap(), state);
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(source.framebuffer(), restored.framebuffer());
    assert_eq!(
        super::super::encode_state(&source).unwrap(),
        super::super::encode_state(&restored).unwrap()
    );

    assert!(inspect_current_native_pce_cd_tas_state(&pce_cd_machine(0x43, 0x15), &state).is_err());
    assert!(inspect_current_native_pce_cd_tas_state(&pce_cd_machine(0x42, 0x16), &state).is_err());
    assert!(
        inspect_current_native_pce_cd_tas_state(
            &pce_cd_machine_with_arcade_card(0x42, 0x15, true),
            &state,
        )
        .is_err()
    );
    let mut memory_base = pce_cd_machine(0x42, 0x15);
    memory_base
        .devices_mut()
        .controller_mut()
        .set_memory_base128_connected(true);
    assert!(inspect_current_native_pce_cd_tas_state(&memory_base, &state).is_err());
}

#[test]
fn arcade_card_cd_state_restores_ram_ports_topology_and_continuation() {
    let mut source = pce_cd_machine_with_arcade_card(0x42, 0x15, true);
    let arcade = source.devices_mut().arcade_card_mut().unwrap();
    arcade.ram_mut()[0x12345] = 0xA5;
    arcade.write_physical(0x1F_FA02, 0xFE);
    arcade.write_physical(0x1F_FA03, 0xFF);
    arcade.write_physical(0x1F_FA04, 0x1F);
    arcade.write_physical(0x1F_FA07, 3);
    arcade.write_physical(0x08_0123, 0x5A);
    advance_frames(&mut source, 3);
    let state = super::super::encode_state(&source).unwrap();
    assert!(inspect_current_native_pce_cd_tas_state(&source, &state).is_err());
    assert!(
        inspect_current_native_pce_cd_tas_state_for_arcade_card(
            &pce_cd_machine(0x42, 0x15),
            &state,
            true,
        )
        .is_err()
    );

    let mut restored = pce_cd_machine_with_arcade_card(0x42, 0x15, true);
    let inspection =
        inspect_current_native_pce_cd_tas_state_for_arcade_card(&restored, &state, true).unwrap();
    assert!(inspection.arcade_card_enabled);
    assert_eq!(
        inspect_current_native_pce_cd_tas_state_identity_for_arcade_card(&state, true)
            .unwrap()
            .disc_sha256,
        inspection.disc_sha256
    );
    assert!(inspect_current_native_pce_cd_tas_state_identity(&state).is_err());
    assert_eq!(
        validate_and_load_current_native_pce_cd_tas_state_for_arcade_card(
            &mut restored,
            &state,
            true,
        )
        .unwrap(),
        inspection.projection
    );
    assert_eq!(super::super::encode_state(&restored).unwrap(), state);
    assert_eq!(
        restored
            .devices()
            .arcade_card()
            .unwrap()
            .peek_physical(0x08_0123),
        Some(0x5A)
    );
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(
        super::super::encode_state(&restored).unwrap(),
        super::super::encode_state(&source).unwrap()
    );
}

#[test]
fn memory_base_cd_state_restores_ram_protocol_topology_and_continuation() {
    let mut source = pce_cd_machine(0x42, 0x15);
    let mut ram = vec![0; MEMORY_BASE128_RAM_LEN];
    for (index, byte) in ram.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(29);
    }
    {
        let controller = source.devices_mut().controller_mut();
        controller.memory_base128_mut().load_ram(&ram).unwrap();
        controller.set_memory_base128_connected(true);
        for bit in [false, false, false, true, false, true, false, true] {
            controller.write_lines(bit, false);
            controller.write_lines(bit, true);
        }
        controller.write_lines(false, false);
        controller.write_lines(false, true);
    }
    advance_frames(&mut source, 3);
    let state = super::super::encode_state(&source).unwrap();

    let mismatched = pce_cd_machine(0x42, 0x15);
    let before = super::super::encode_state(&mismatched).unwrap();
    assert!(
        inspect_current_native_pce_cd_tas_state_for_profile(&mismatched, &state, false, false,)
            .is_err()
    );
    assert_eq!(super::super::encode_state(&mismatched).unwrap(), before);

    let mut restored = pce_cd_machine(0x42, 0x15);
    restored
        .devices_mut()
        .controller_mut()
        .set_memory_base128_connected(true);
    let inspection =
        inspect_current_native_pce_cd_tas_state_for_profile(&restored, &state, false, true)
            .unwrap();
    assert!(inspection.memory_base_enabled);
    assert_eq!(
        validate_and_load_current_native_pce_cd_tas_state_for_profile(
            &mut restored,
            &state,
            false,
            true,
        )
        .unwrap(),
        inspection.projection
    );
    let memory_base = restored.devices().controller().memory_base128();
    assert!(memory_base.is_connected());
    assert_eq!(memory_base.ram(), ram);
    assert_eq!(super::super::encode_state(&restored).unwrap(), state);
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(
        super::super::encode_state(&restored).unwrap(),
        super::super::encode_state(&source).unwrap()
    );
}

#[test]
fn sf2_state_restores_exact_identity_output_and_continuation() {
    let mut image = vec![0xEA; crate::hardware::SF2_CE_HUCARD_IMAGE_LEN];
    image[..6].copy_from_slice(&[0xA9, 0x00, 0x8D, 0xF3, 0x1F, 0xEA]);
    image[0x1FFE] = 0;
    image[0x1FFF] = 0;
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Sf2Ce);
    let mut source = PceMachine::with_cartridge_and_controller(
        image.clone(),
        descriptor,
        ControllerPort::two_button(),
    )
    .unwrap();
    source.set_sample_generation_enabled(false);
    advance_frames(&mut source, 2);
    let state = super::super::encode_state(&source).unwrap();
    let mut restored =
        PceMachine::with_cartridge_and_controller(image, descriptor, ControllerPort::two_button())
            .unwrap();
    restored.set_sample_generation_enabled(false);

    let inspection = inspect_current_native_direct_hucard_tas_state_for_board(
        &restored,
        &state,
        PceHuCardBoard::Sf2Ce,
    )
    .unwrap();
    assert_eq!(inspection.board, PceHuCardBoard::Sf2Ce);
    assert_eq!(inspection.projection.frame_count, 2);
    assert_eq!(
        inspect_current_native_supported_hucard_tas_state_identity(&state)
            .unwrap()
            .board,
        PceHuCardBoard::Sf2Ce
    );

    let projection = validate_and_load_current_native_direct_hucard_tas_state_for_board(
        &mut restored,
        &state,
        PceHuCardBoard::Sf2Ce,
    )
    .unwrap();
    assert_eq!(projection, inspection.projection);
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(source.framebuffer(), restored.framebuffer());
    assert_eq!(
        super::super::encode_state(&source).unwrap(),
        super::super::encode_state(&restored).unwrap()
    );
}

#[test]
fn populous_state_restores_mapper_ram_identity_output_and_continuation() {
    let mut image = vec![0xEA; crate::hardware::POPULOUS_HUCARD_IMAGE_LEN];
    image[..12].copy_from_slice(&[
        0xA9, 0x40, 0x53, 0x02, 0xA9, 0xA5, 0x8D, 0x00, 0x20, 0x4C, 0x09, 0x00,
    ]);
    image[0x1FFE] = 0;
    image[0x1FFF] = 0;
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Populous);
    let mut source = PceMachine::with_cartridge_and_controller(
        image.clone(),
        descriptor,
        ControllerPort::two_button(),
    )
    .unwrap();
    source.set_sample_generation_enabled(false);
    advance_frames(&mut source, 4);
    assert_eq!(source.hucard_ram().unwrap()[0], 0xA5);
    let state = super::super::encode_state(&source).unwrap();
    let mut restored =
        PceMachine::with_cartridge_and_controller(image, descriptor, ControllerPort::two_button())
            .unwrap();
    restored.set_sample_generation_enabled(false);

    let inspection = inspect_current_native_direct_hucard_tas_state_for_board(
        &restored,
        &state,
        PceHuCardBoard::Populous,
    )
    .unwrap();
    assert_eq!(inspection.board, PceHuCardBoard::Populous);
    assert_eq!(inspection.projection.frame_count, 4);
    assert_eq!(
        inspect_current_native_supported_hucard_tas_state_identity(&state)
            .unwrap()
            .board,
        PceHuCardBoard::Populous
    );

    let projection = validate_and_load_current_native_direct_hucard_tas_state_for_board(
        &mut restored,
        &state,
        PceHuCardBoard::Populous,
    )
    .unwrap();
    assert_eq!(projection, inspection.projection);
    assert_eq!(restored.hucard_ram().unwrap()[0], 0xA5);
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(source.framebuffer(), restored.framebuffer());
    assert_eq!(
        super::super::encode_state(&source).unwrap(),
        super::super::encode_state(&restored).unwrap()
    );
}

#[test]
fn supergrafx_state_restores_dual_video_identity_output_and_continuation() {
    let image = rom(0x42);
    let descriptor =
        PceCartridgeDescriptor::default().with_required_hardware(PceCartridgeHardware::SuperGrafx);
    let mut source = PceMachine::with_cartridge_and_controller(
        image.clone(),
        descriptor,
        ControllerPort::two_button(),
    )
    .unwrap();
    source.set_sample_generation_enabled(false);
    advance_frames(&mut source, 3);
    let state = super::super::encode_state(&source).unwrap();
    let mut restored =
        PceMachine::with_cartridge_and_controller(image, descriptor, ControllerPort::two_button())
            .unwrap();
    restored.set_sample_generation_enabled(false);

    let inspection = inspect_current_native_direct_hucard_tas_state_for_profile(
        &restored,
        &state,
        PceHuCardBoard::Plain,
        PceHardwareTopology::SuperGrafx,
    )
    .unwrap();
    assert_eq!(inspection.board, PceHuCardBoard::Plain);
    assert_eq!(inspection.topology, PceHardwareTopology::SuperGrafx);
    assert_eq!(inspection.psg_revision, PsgRevision::HuC6280A);
    assert_eq!(inspection.projection.frame_count, 3);
    let identity = inspect_current_native_supported_hucard_tas_state_identity(&state).unwrap();
    assert_eq!(identity.board, PceHuCardBoard::Plain);
    assert_eq!(identity.topology, PceHardwareTopology::SuperGrafx);

    let projection = validate_and_load_current_native_direct_hucard_tas_state_for_profile(
        &mut restored,
        &state,
        PceHuCardBoard::Plain,
        PceHardwareTopology::SuperGrafx,
    )
    .unwrap();
    assert_eq!(projection, inspection.projection);
    advance_frames(&mut source, 1);
    advance_frames(&mut restored, 1);
    assert_eq!(source.framebuffer(), restored.framebuffer());
    assert_eq!(
        super::super::encode_state(&source).unwrap(),
        super::super::encode_state(&restored).unwrap()
    );
}

#[test]
fn six_button_state_restores_transactionally_and_continues() {
    let image = rom(0x53);
    let descriptor = PceCartridgeDescriptor::default();
    let mut source = PceMachine::with_cartridge_and_controller(
        image.clone(),
        descriptor,
        ControllerPort::six_button(),
    )
    .unwrap();
    source
        .devices_mut()
        .controller_mut()
        .six_button_pad_mut()
        .unwrap()
        .set_extra_buttons(
            crate::hardware::SixButtonExtraButtons::III
                | crate::hardware::SixButtonExtraButtons::VI,
        );
    source
        .devices_mut()
        .controller_mut()
        .write_lines(false, false);
    source
        .devices_mut()
        .controller_mut()
        .write_lines(true, false);
    let state = super::super::encode_state(&source).unwrap();
    let mut target =
        PceMachine::with_cartridge_and_controller(image, descriptor, ControllerPort::six_button())
            .unwrap();
    let inspection = inspect_current_native_direct_hucard_tas_state_for_profile_and_controller(
        &target,
        &state,
        PceHuCardBoard::Plain,
        PceHardwareTopology::Base,
        PceControllerMode::SixButton,
    )
    .unwrap();
    assert_eq!(
        inspection.controller_extra_buttons,
        crate::hardware::SixButtonExtraButtons::III | crate::hardware::SixButtonExtraButtons::VI
    );
    validate_and_load_current_native_direct_hucard_tas_state_for_profile_and_controller(
        &mut target,
        &state,
        PceHuCardBoard::Plain,
        PceHardwareTopology::Base,
        PceControllerMode::SixButton,
    )
    .unwrap();
    assert_eq!(super::super::encode_state(&target).unwrap(), state);
}

#[test]
fn invalid_states_and_topologies_reject_atomically() {
    let image = rom(0xEA);
    let source = machine(image.clone());
    let current = super::super::encode_state(&source).unwrap();
    let mut target = machine(image.clone());
    advance_frames(&mut target, 1);
    let before = super::super::encode_state(&target).unwrap();

    let mut legacy = current.clone();
    legacy[8..12].copy_from_slice(&(PCE_SAVE_STATE_FORMAT_VERSION - 1).to_le_bytes());
    let mut trailing = current.clone();
    trailing.push(0);
    let truncated = &current[..current.len() - 1];
    for invalid in [&legacy[..], &trailing, truncated] {
        assert!(
            validate_and_load_current_native_direct_hucard_tas_state(&mut target, invalid).is_err()
        );
        assert_eq!(super::super::encode_state(&target).unwrap(), before);
    }

    let wrong_rom = super::super::encode_state(&machine(rom(0xA9))).unwrap();
    assert!(
        validate_and_load_current_native_direct_hucard_tas_state(&mut target, &wrong_rom).is_err()
    );
    assert_eq!(super::super::encode_state(&target).unwrap(), before);

    let descriptor =
        PceCartridgeDescriptor::default().with_required_hardware(PceCartridgeHardware::SuperGrafx);
    let supergrafx = PceMachine::with_cartridge(image.clone(), descriptor).unwrap();
    assert!(inspect_current_native_direct_hucard_tas_state(&supergrafx, &current).is_err());

    let mut memory_base = machine(image.clone());
    memory_base
        .devices_mut()
        .controller_mut()
        .set_memory_base128_connected(true);
    let memory_base_state = super::super::encode_state(&memory_base).unwrap();
    assert!(
        validate_and_load_current_native_direct_hucard_tas_state(&mut target, &memory_base_state,)
            .is_err()
    );
    assert_eq!(super::super::encode_state(&target).unwrap(), before);

    let mut six_button = PceMachine::with_controller(image, ControllerPort::six_button()).unwrap();
    six_button
        .devices_mut()
        .controller_mut()
        .six_button_pad_mut()
        .unwrap()
        .set_extra_buttons(SixButtonExtraButtons::III);
    let six_button_state = super::super::encode_state(&six_button).unwrap();
    assert!(
        validate_and_load_current_native_direct_hucard_tas_state(&mut target, &six_button_state,)
            .is_err()
    );
    assert_eq!(super::super::encode_state(&target).unwrap(), before);
}
