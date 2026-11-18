use super::*;

#[test]
fn pce_libretro_roundtrips_serialize_payload_above_four_mib() {
    let mut state = CoreState::from_rom(&pce_rom(), "state.pce").unwrap();
    state.set_input(0x0F, 0x09);
    state.step_frame();
    let encoded = state.encode_state().unwrap();
    assert!(encoded.len() > 4 * 1024 * 1024);
    let expected_frame = match &mut state.core {
        ActiveCore::Pce(host) => host.framebuffer().to_vec(),
        _ => unreachable!(),
    };

    state.reset().unwrap();
    state.load_state(&encoded).unwrap();

    assert_eq!(state.encode_state().unwrap(), encoded);
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    assert_eq!(host.framebuffer(), expected_frame);
}

#[test]
fn failed_unserialize_preserves_continuing_session() {
    let mut source = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    source.set_input(0x03, 0x05);
    source.step_frame();
    let checkpoint = source.encode_state().unwrap();
    let mut control = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    let mut target = CoreState::from_rom(&gba_rom(), "rollback.gba").unwrap();
    control.load_state(&checkpoint).unwrap();
    target.load_state(&checkpoint).unwrap();
    let before_failed = target.encode_state().unwrap();
    assert_eq!(control.encode_state().unwrap(), before_failed);
    let mut invalid = checkpoint.clone();
    invalid.push(0xA5);

    assert!(target.load_state(&invalid).is_err());
    assert_eq!(target.encode_state().unwrap(), before_failed);
    control.step_frame();
    target.step_frame();
    control.drain_audio();
    target.drain_audio();
    assert_eq!(
        target.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    assert_eq!(
        target.framebuffer_as_xrgb8888(),
        control.framebuffer_as_xrgb8888()
    );
    assert_eq!(target.audio_buf, control.audio_buf);
}

#[test]
fn pce_libretro_abi_serialization_roundtrips_above_four_mib() {
    let _abi = crate::callbacks::lock(&crate::callbacks::ABI_TEST_LOCK);
    let mut state = CoreState::from_rom(&pce_rom(), "abi-state.pce").unwrap();
    state.step_frame();
    let expected = state.encode_state().unwrap();
    assert!(expected.len() > 4 * 1024 * 1024);
    *crate::callbacks::lock(&crate::callbacks::CORE) = Some(state);
    *crate::callbacks::lock(&crate::callbacks::MAX_SERIALIZE_SIZE) = 0;

    let serialize_size = crate::serialization::retro_serialize_size();
    assert!(serialize_size > expected.len());
    let mut buffer = vec![0; serialize_size];
    assert!(!crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        buffer.len() - 1
    ));
    assert!(crate::serialization::retro_serialize(
        buffer.as_mut_ptr().cast(),
        buffer.len()
    ));
    crate::callbacks::lock(&crate::callbacks::CORE)
        .as_mut()
        .unwrap()
        .reset()
        .unwrap();
    assert!(crate::serialization::retro_unserialize(
        buffer.as_ptr().cast(),
        buffer.len()
    ));

    let restored = crate::callbacks::lock(&crate::callbacks::CORE)
        .as_ref()
        .unwrap()
        .encode_state()
        .unwrap();
    assert_eq!(restored, expected);
    *crate::callbacks::lock(&crate::callbacks::CORE) = None;
}

#[test]
fn pce_libretro_forwards_two_button_player_one_input() {
    let mut state = CoreState::from_rom(&pce_rom(), "input.pce").unwrap();
    state.set_input(0x0F, 0x09);

    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    let zeff_pce_core::hardware::ControllerDevice::TwoButton(pad) =
        host.machine().devices().controller().device()
    else {
        panic!("expected two-button controller");
    };
    let expected = zeff_pce_core::hardware::PadButtons::I
        | zeff_pce_core::hardware::PadButtons::II
        | zeff_pce_core::hardware::PadButtons::SELECT
        | zeff_pce_core::hardware::PadButtons::RUN
        | zeff_pce_core::hardware::PadButtons::RIGHT
        | zeff_pce_core::hardware::PadButtons::DOWN;
    assert_eq!(pad.buttons(), expected);
}

#[test]
fn pce_libretro_applies_logical_ram_cheats() {
    let mut state = CoreState::from_rom(&pce_rom(), "logical-cheat.pce").unwrap();
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    host.machine_mut()
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0xF8);

    state.cheat_set("4005:42");
    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        unreachable!();
    };
    assert_eq!(host.machine().mapped_work_ram()[5], 0x42);
}

#[test]
fn pce_libretro_applies_six_digit_physical_work_ram_cheats() {
    let mut state = CoreState::from_rom(&pce_rom(), "physical-cheat.pce").unwrap();

    state.cheat_set("1F2345:66");
    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        panic!("expected PCE host");
    };
    assert_eq!(host.machine().mapped_work_ram()[0x345], 0x66);
}

#[test]
fn pce_libretro_cheat_reset_stops_logical_and_physical_writes() {
    let mut state = CoreState::from_rom(&pce_rom(), "reset-cheats.pce").unwrap();
    let ActiveCore::Pce(host) = &mut state.core else {
        panic!("expected PCE host");
    };
    host.machine_mut()
        .cpu_mut()
        .cpu_mut()
        .set_mapping_register(2, 0xF8);
    state.cheat_set("4005:42 + 1F2345:66");
    state.cheat_reset();

    state.apply_ram_cheats();

    let ActiveCore::Pce(host) = &state.core else {
        unreachable!();
    };
    assert_eq!(host.machine().mapped_work_ram()[5], 0);
    assert_eq!(host.machine().mapped_work_ram()[0x345], 0);
}
