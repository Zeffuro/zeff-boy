use super::*;

fn sound(machine: &mut Emulator, program: &[u8]) {
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
}

#[test]
fn cached_field_returns_exact_raw17_and_public_instruction_continuation() {
    let mut actual = fixture(
        Region::NtscUs,
        true,
        &[0x33fc, 0x1234, 0x00ff, 0x0040, 0x60f6],
    );
    sound(&mut actual, &[0x21, 0x00, 0x01, 0x7e, 0x3c, 0x18, 0xfc]);
    let mut expected = actual.clone();
    for _ in 0..2 {
        compare_field(&mut actual, &mut expected).0.unwrap();
        assert!(actual.sound_schedule.epoch.is_none());
        let raw = actual.encode_state().unwrap();
        let expected_raw = expected.encode_state().unwrap();
        assert_eq!(raw, expected_raw);
        actual.decode_state(&raw).unwrap();
        expected.decode_state(&expected_raw).unwrap();
        assert_eq!(actual.encode_state().unwrap(), raw);
        assert_eq!(expected.encode_state().unwrap(), expected_raw);
        assert_observations(&actual, &expected);
        for _ in 0..17 {
            assert_eq!(actual.step_instruction(), expected.step_instruction());
            assert!(actual.sound_schedule.epoch.is_none());
            assert_observations(&actual, &expected);
        }
    }
}

#[test]
fn public_fault_returns_materialize_both_main_and_sound_prefixes() {
    let mut main_program = vec![0x4e71; 32];
    main_program.extend([0x3039, 0x0080, 0x0000]);
    let mut actual = fixture(Region::NtscUs, true, &main_program);
    sound(&mut actual, &[0; 16]);
    let mut expected = actual.clone();
    assert!(matches!(
        compare_field(&mut actual, &mut expected).0,
        Err(MachineFault::Main(_))
    ));
    assert!(actual.sound_schedule.epoch.is_none());

    let mut actual = fixture(Region::NtscUs, true, &[0x60fe, 0x4e71]);
    sound(&mut actual, &[0, 0, 0, 0, 0x3a, 0x00, 0x7f]);
    let mut expected = actual.clone();
    assert!(matches!(
        compare_field(&mut actual, &mut expected).0,
        Err(MachineFault::Sound(SoundFault::UnsupportedAccess { .. }))
    ));
    assert!(actual.sound_schedule.epoch.is_none());
}

#[test]
fn traced_field_uses_ordinary_path_with_sound_continuations() {
    let mut actual = fixture(Region::NtscUs, true, &[0x60fe, 0x4e71]);
    actual.reset_and_begin_audio_trace(64).unwrap();
    sound(&mut actual, &[0x21, 0x00, 0x01, 0x7e, 0x3c, 0x18, 0xfc]);
    let mut expected = actual.clone();
    compare_field(&mut actual, &mut expected).0.unwrap();
    assert!(actual.sound_schedule.epoch.is_none());
    assert_eq!(actual.finish_audio_trace(), expected.finish_audio_trace());
}

#[test]
fn vdp_polling_fields_keep_exact_public_state_and_output() {
    for port in [0x0004, 0x0008] {
        let mut actual = fixture(Region::NtscUs, true, &[0x3039, 0x00c0, port, 0x60f8]);
        sound(&mut actual, &[0x21, 0x00, 0x01, 0x7e, 0x3c, 0x18, 0xfc]);
        let mut expected = actual.clone();
        compare_field(&mut actual, &mut expected).0.unwrap();
        assert!(actual.sound_schedule.epoch.is_none());
    }
}

#[test]
fn public_vdp_read_fault_retains_exact_sound_prefix_and_public_state_cut() {
    let mut program = vec![0x4e71; 32];
    program.extend([0x3039, 0x00c0, 0x0000]);
    let mut actual = fixture(Region::NtscUs, true, &program);
    sound(&mut actual, &[0; 16]);
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(MasterTicks::new(1), Region::NtscUs).unwrap();
    vdp.write_control(0x8f02, Region::NtscUs).unwrap();
    vdp.advance_to(actual.bus.now, Region::NtscUs).unwrap();
    vdp.write_control(0x6000, Region::NtscUs).unwrap();
    vdp.write_control(0x0000, Region::NtscUs).unwrap();
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        vdp.write_data(word, Region::NtscUs).unwrap();
    }
    vdp.write_control(0x004e, Region::NtscUs).unwrap();
    vdp.write_control(0x0010, Region::NtscUs).unwrap();
    actual.bus.vdp = vdp;
    let before = actual.bus.now;
    let mut expected = actual.clone();
    assert!(matches!(
        compare_field(&mut actual, &mut expected).0,
        Err(MachineFault::Main(TimingFault::Bus {
            source: BusError::VdpPort(crate::VdpPortError::Read(
                crate::VdpReadError::UnsupportedCommand(_)
            )),
            ..
        }))
    ));
    assert!(actual.bus.now > before);
    assert!(actual.sound_cpu.cycles() > 0);
    assert!(actual.sound_schedule.epoch.is_none());
    let actual_error = actual.encode_state().unwrap_err().to_string();
    assert_eq!(
        actual_error,
        expected.encode_state().unwrap_err().to_string()
    );
    assert_eq!(
        actual_error,
        "M68000 state requires an ordinary execution boundary"
    );
}
