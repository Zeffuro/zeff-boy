use super::*;

fn program(region: Region, concurrent_sound: bool, words: &[u16]) -> Emulator {
    let mut bytes = vec![0; 0x1000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in words.iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn idle(region: Region, concurrent: bool) -> Emulator {
    let mut result = program(
        region,
        concurrent,
        &[
            0x13fc, 0x20, 0x00a1, 0x0013, 0x13fc, 0x20, 0x00a1, 0x0019, 0x13fc, 0xfc, 0x00a1,
            0x0009, 0x13fc, 0xfc, 0x00a1, 0x000b, 0x13fc, 0x5f, 0x00a1, 0x0003, 0x13fc, 0x5f,
            0x00a1, 0x0005, 0x4e71, 0x60fc,
        ],
    );
    for _ in 0..6 {
        result.step_instruction().unwrap();
    }
    result
}

pub(super) fn futures() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let source = idle(region, concurrent);
            assert_eq!(source.peek(0xa10013), Some(0x20));
            assert_eq!(source.peek(0xa10019), Some(0x20));
            assert_ne!(source.peek(0xa10003).unwrap() & 0x20, 0);
            assert_ne!(source.peek(0xa10005).unwrap() & 0x20, 0);
            assert_eq!(source.peek(0xa1000f), None);
            assert_eq!(source.peek(0xa10011), None);
            assert!(source.check_warm_reset().is_err());
            assert!(source.clone().reset_cpu().is_err());
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
            let mut reader = StateReader::new(&raw);
            reader.set_position(52);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let bus_at = reader.position() + 4;
            let bus = reader.read_slice(MAX_STATE_BYTES).unwrap();
            let mut bus_reader = StateReader::new(bus);
            layout::bus_prefix(&mut bus_reader);
            let input_at = bus_at + bus_reader.position() + 4;
            let input = bus_reader.read_slice(MAX_STATE_BYTES).unwrap();
            let uart_at = input_at + layout::uart_history_offset(input);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            reader.read_bool().unwrap();
            for _ in 0..3 {
                reader.read_slice(MAX_STATE_BYTES).unwrap();
            }
            let capabilities = reader.position() + 4;
            for port in [ControllerPort::One, ControllerPort::Two] {
                let mut target = source.clone();
                let at = target.bus.now.get();
                target.input_mut().set_controller_raw(port, 0xdf);
                let fault = target.input_fault().unwrap();
                assert_eq!(fault.port, port);
                assert_eq!(fault.at, at);
                target.input_mut().set_controller_raw(port, 0xff);
                let before = target.clone();
                assert_eq!(target.step_instruction(), Err(MachineFault::Input(fault)));
                assert_eq!(target, before);
                assert_eq!(target.step_field(), Err(MachineFault::Input(fault)));
                assert_eq!(target, before);
                assert_eq!(
                    target.step_sound_instruction(),
                    Err(SoundFault::InputBlocked(fault))
                );
                assert_eq!(target, before);
                assert_eq!(target.peek(0xa10003), None);
                assert!(target.encode_state().is_err());
                assert!(target.encode_frontend_state().is_err());
                for end in [0, 12, raw.len() - 1] {
                    assert!(target.decode_state(&raw[..end]).is_err());
                    assert_eq!(target, before);
                }
                for offset in [
                    uart_at,
                    uart_at + 1,
                    capabilities,
                    capabilities + 1,
                    capabilities + 2,
                    capabilities + 3,
                ] {
                    let mut invalid = raw.clone();
                    invalid[offset] = 3;
                    assert!(target.decode_state(&invalid).is_err());
                    assert_eq!(target, before);
                }
                let mut late = front.clone();
                let mut reader = StateReader::new(&late);
                reader.set_position(12);
                reader.read_slice(MAX_STATE_BYTES).unwrap();
                let audio = reader.position() + 4;
                late[audio..audio + 4].fill(0);
                assert!(target.decode_frontend_state(&late).is_err());
                assert_eq!(target, before);
                for frontend in [false, true] {
                    let mut restored = before.clone();
                    let mut control = source.clone();
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                        control.decode_frontend_state(&front).unwrap();
                    } else {
                        restored.decode_state(&raw).unwrap();
                        control.decode_state(&raw).unwrap();
                    }
                    assert_eq!(restored.input_fault(), None);
                    for _ in 0..8 {
                        assert_eq!(restored.step_instruction(), control.step_instruction());
                    }
                    assert_eq!(
                        (&restored.encode_state().unwrap()),
                        (&control.encode_state().unwrap())
                    );
                    assert_eq!(
                        restored.drain_audio_samples(),
                        control.drain_audio_samples()
                    );
                }
            }
            guest_guards(region, concurrent);
            output_futures(region, concurrent);
        }
    }
}

fn output_program(region: Region, concurrent: bool) -> Emulator {
    program(
        region,
        concurrent,
        &[
            0x303c, 1024, 0x51c8, 0xfffe, 0x13fc, 0x20, 0x00a1, 0x0013, 0x13fc, 0x20, 0x00a1,
            0x0019, 0x13fc, 0x38, 0x00a1, 0x0013, 0x13fc, 0x38, 0x00a1, 0x0019, 0x4e71, 0x60fc,
        ],
    )
}

fn output_futures(region: Region, concurrent: bool) {
    output_admission(region);
    let mut source = output_program(region, concurrent);
    for _ in 0..1100 {
        if source.peek(0xa10019) == Some(0x20) {
            break;
        }
        source.step_instruction().unwrap();
    }
    assert!(source.bus.now.get() >= 24_206);
    assert_eq!(source.peek(0xa10013), Some(0x20));
    assert_eq!(source.peek(0xa10019), Some(0x20));

    source.step_instruction().unwrap();
    source.step_instruction().unwrap();
    let raw = source.encode_state().unwrap();
    assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
    let front = source.encode_frontend_state().unwrap();
    let mut frontend = output_program(region, concurrent);
    frontend.decode_frontend_state(&front).unwrap();
    let mut reference = source.clone();
    reference.decode_state(&raw).unwrap();
    let mut restored = output_program(region, concurrent);
    restored.decode_state(&raw).unwrap();
    assert_eq!(frontend.encode_frontend_state().unwrap(), front);
    assert_eq!(restored.encode_state().unwrap(), raw);
    assert!(frontend.check_warm_reset().is_err());
    assert!(frontend.reset_cpu().is_err());
    assert_eq!(frontend.encode_frontend_state().unwrap(), front);
    let before = restored.encode_frontend_state().unwrap();
    let mut wrong_owner = raw.clone();
    *wrong_owner.last_mut().unwrap() = 1;
    assert!(restored.decode_state(&wrong_owner).is_err());
    assert_eq!(restored.encode_frontend_state().unwrap(), before);
    let mut outer = StateReader::new(&raw);
    outer.set_position(52);
    outer.read_slice(MAX_STATE_BYTES).unwrap();
    let bus_at = outer.position() + 4;
    let bus = outer.read_slice(MAX_STATE_BYTES).unwrap();
    let mut bus_reader = StateReader::new(bus);
    layout::bus_prefix(&mut bus_reader);
    let input_at = bus_at + bus_reader.position() + 4;
    let mut low_tl = raw.clone();
    low_tl[input_at + 1] = 0xef;
    assert!(restored.decode_state(&low_tl).is_err());
    assert_eq!(restored.encode_frontend_state().unwrap(), before);
    let mut late = front.clone();
    let mut outer = StateReader::new(&late);
    outer.set_position(12);
    outer.read_slice(MAX_STATE_BYTES).unwrap();
    let audio_at = outer.position() + 4;
    late[audio_at..audio_at + 4].fill(0);
    assert!(restored.decode_frontend_state(&late).is_err());
    assert_eq!(restored.encode_frontend_state().unwrap(), before);
    for _ in 0..16 {
        for machine in [&mut source, &mut frontend, &mut reference, &mut restored] {
            machine.step_instruction().unwrap();
            assert_eq!(machine.peek(0xa10013), Some(0x38));
            assert_eq!(machine.peek(0xa10019), Some(0x38));
            assert_eq!(machine.peek(0xa10003).unwrap() & 0x30, 0x30);
        }
        assert_eq!(
            source.encode_state().unwrap(),
            reference.encode_state().unwrap()
        );
        assert_eq!(source.framebuffer(), reference.framebuffer());

        for (left, right) in [
            (&mut source, &mut frontend),
            (&mut reference, &mut restored),
        ] {
            assert_eq!(
                left.encode_frontend_state().unwrap(),
                right.encode_frontend_state().unwrap()
            );
            assert_eq!(left.encode_state().unwrap(), right.encode_state().unwrap());
            let a = left.drain_audio_samples();
            let b = right.drain_audio_samples();
            assert_eq!(a.len(), b.len());
            assert!(a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }
    frontend
        .input_mut()
        .set_controller_raw(ControllerPort::One, 0xef);
    let fault = frontend.input_fault().unwrap();
    let clock = frontend.bus.now;
    let registers = frontend.registers();
    frontend
        .input_mut()
        .set_controller_raw(ControllerPort::One, 0xff);
    assert_eq!(frontend.step_instruction(), Err(MachineFault::Input(fault)));
    assert_eq!(frontend.step_field(), Err(MachineFault::Input(fault)));
    assert_eq!(
        frontend.step_sound_instruction(),
        Err(SoundFault::InputBlocked(fault))
    );
    assert_eq!(frontend.bus.now, clock);
    assert_eq!(frontend.registers(), registers);
    assert!(frontend.encode_state().is_err());
    frontend.decode_frontend_state(&front).unwrap();
    assert_eq!(frontend.input_fault(), None);
    assert_eq!(frontend.encode_frontend_state().unwrap(), front);
}

fn output_admission(region: Region) {
    for port in 0..2 {
        let base = port * 3;
        let mut input = crate::hardware::input::Input::cold();
        assert!(input.write_serial(base + 2, 0x20, 0, region));
        let before = input.clone();
        assert!(!input.write_serial(base + 2, 0x38, 24_205, region));
        assert_eq!(input, before);
        assert!(input.write_serial(base + 2, 0x38, 24_206, region));
        assert_eq!(input.read_serial(base + 2, 24_206, region), 0x38);
        assert!(!input.serial_read_allowed(base));
        assert!(!input.serial_read_allowed(base + 1));
        assert!(input.write_control(port, 0x10, 24_206, region));
        assert!(input.write_data(port, 0, 24_206, region));
        assert_eq!(input.read_data(port, 24_206, region) & 0x30, 0x30);
        for value in [0, 0x20, 0x28, 0x78] {
            let before = input.clone();
            assert!(!input.write_serial(base + 2, value, 24_206, region));
            assert_eq!(input, before);
        }
        assert!(input.validate_serial(24_205, region).is_err());
        let mut writer = StateWriter::new();
        assert!(input.write_state(&mut writer, 24_205).is_err());
        assert!(writer.into_bytes().is_empty());
        let mut writer = StateWriter::new();
        input.write_state(&mut writer, 24_206).unwrap();
        let bytes = writer.into_bytes();
        let mut copy = crate::hardware::input::Input::cold();
        assert!(
            copy.read_state(&mut StateReader::new(&bytes), 24_205, region)
                .is_err()
        );
        assert!(input.write_data(port, 0x10, 24_206, region));
        assert!(input.write_serial(base + 2, 0x20, 24_206, region));
        assert!(input.write_serial(base + 2, 0x38, 24_206, region));
        assert!(input.write_serial(base + 2, 0, 24_206, region));
    }
}

fn guest_guards(region: Region, concurrent: bool) {
    for port in 0..2u16 {
        let status = 0x13 + port * 6;
        for (opcode, address, value) in [
            (0x1039, status - 2, 0),
            (0x1039, status - 4, 0),
            (0x13fc, status - 4, 0xff),
            (0x13fc, status, 0x38),
            (0x4e70, 0, 0),
        ] {
            let mut words = vec![0x13fc, 0x20, 0x00a1, status];
            if opcode == 0x4e70 {
                words.push(opcode);
            } else if opcode == 0x1039 {
                words.extend([opcode, 0x00a1, address]);
            } else {
                words.extend([opcode, value, 0x00a1, address]);
            }
            words.extend([0x13fc, 0x5a, 0x00ff, 0x0100, 0x60fe]);
            let mut machine = program(region, concurrent, &words);
            machine.step_instruction().unwrap();
            let input = machine.input().clone();
            assert!(matches!(
                machine.step_instruction(),
                Err(MachineFault::Main(_))
            ));
            assert_eq!(machine.input(), &input);
            assert_eq!(machine.peek(0xff0100), Some(0));
            let failed = machine.clone();
            assert_eq!(
                machine.step_instruction(),
                Err(MachineFault::Main(TimingFault::Frozen))
            );
            assert_eq!(machine, failed);
        }
        let status = 0x13 + port * 6;
        let mut machine = program(
            region,
            concurrent,
            &[
                0x13fc,
                0xff,
                0x00a1,
                status - 4,
                0x13fc,
                0x20,
                0x00a1,
                status,
                0x60fe,
            ],
        );
        machine.step_instruction().unwrap();
        assert!(matches!(
            machine.step_instruction(),
            Err(MachineFault::Main(_))
        ));
        let words = [
            0x13fc,
            0x40,
            0x00a1,
            0x09 + port * 2,
            0x13fc,
            0x20,
            0x00a1,
            status,
            0x13fc,
            0,
            0x00a1,
            0x03 + port * 2,
            0x60fe,
        ];
        let mut machine = program(region, concurrent, &words);
        let key = if port == 0 {
            ControllerPort::One
        } else {
            ControllerPort::Two
        };
        machine.input_mut().set_controller_raw(key, 0x7f);
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        let input = machine.input().clone();
        assert!(matches!(
            machine.step_instruction(),
            Err(MachineFault::Main(_))
        ));
        assert_eq!(machine.input(), &input);
    }
}
