use super::*;

fn program(region: Region, concurrent_sound: bool) -> Emulator {
    let mut words = vec![0x4e71_u16; 1000];
    words.extend_from_slice(&[
        0x13fc, 0x38, 0x00a1, 0x0019, 0x13fc, 0x53, 0x00a1, 0x0015, 0x1039, 0x00a1, 0x0019, 0x0839,
        0, 0x00a1, 0x0019, 0x66f6, 0x13fc, 0x4d, 0x00a1, 0x0015, 0x0839, 0, 0x00a1, 0x0019, 0x66f6,
        0x746b, 0x60fe,
    ]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

pub(super) fn futures() {
    inactive_input_futures();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut source = program(region, concurrent);
            for _ in 0..1001 {
                source.step_instruction().unwrap();
            }
            assert_eq!(source.registers().pc, 0x9d8);
            assert_eq!(source.peek(0xa10019), Some(0x38));
            source.step_instruction().unwrap();
            assert_eq!(source.peek(0xa10019), None);
            assert_eq!(source.peek(0xa10005), None);
            assert_eq!(source.peek(0xa10015), None);
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            for frontend in [false, true] {
                let mut control = source.clone();
                let mut restored = program(region, concurrent);
                if frontend {
                    restored.decode_frontend_state(&front).unwrap();
                } else {
                    control.decode_state(&raw).unwrap();
                    restored.decode_state(&raw).unwrap();
                }
                assert_eq!(restored.encode_state().unwrap(), raw);
                for _ in 0..4096 {
                    assert_eq!(
                        restored.step_instruction().unwrap(),
                        control.step_instruction().unwrap()
                    );
                    if control.registers().d[2] == 0x6b {
                        break;
                    }
                }
                assert_eq!(control.registers().d[0] & 0xff, 0x39);
                assert_eq!(control.registers().d[2], 0x6b);
                assert_eq!(
                    restored.encode_state().unwrap(),
                    control.encode_state().unwrap()
                );
                assert_eq!(
                    restored.drain_audio_samples(),
                    control.drain_audio_samples()
                );
                assert!(restored.check_warm_reset().is_err());
            }
        }
    }
}

fn inactive_input_futures() {
    use crate::{ControllerKind, ControllerPort};
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for stage in 0..3 {
                let mut source = program(region, concurrent);
                source.bus.vdp.write_control(0x8104, region).unwrap();
                source
                    .input_mut()
                    .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
                for _ in 0..1002 {
                    source.step_instruction().unwrap();
                }
                assert_eq!(source.registers().pc, 0x9e0);
                if stage != 0 {
                    for _ in 0..4096 {
                        if source.registers().pc == 0x9f0 {
                            break;
                        }
                        source.step_instruction().unwrap();
                    }
                    assert_eq!(source.registers().pc, 0x9f0);
                    if stage == 2 {
                        source.step_instruction().unwrap();
                        assert_eq!(source.registers().pc, 0x9f8);
                    }
                }
                source
                    .input_mut()
                    .set_controller_raw(ControllerPort::One, 0xfd);
                assert_eq!(source.input_fault(), None);
                assert_eq!(source.peek(0xa10003).unwrap() & 2, 0);
                let raw = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                for frontend in [false, true] {
                    let mut control = source.clone();
                    let mut restored = program(region, concurrent);
                    restored
                        .input_mut()
                        .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                    } else {
                        control.decode_state(&raw).unwrap();
                        restored.decode_state(&raw).unwrap();
                    }
                    assert_eq!(
                        control.encode_frontend_state().unwrap(),
                        restored.encode_frontend_state().unwrap()
                    );
                    for step in 0..4096 {
                        if step == 8 {
                            for machine in [&mut control, &mut restored] {
                                machine
                                    .input_mut()
                                    .set_controller_raw(ControllerPort::One, 0xff);
                                assert_eq!(machine.input_fault(), None);
                                assert_ne!(machine.peek(0xa10003).unwrap() & 2, 0);
                            }
                        }
                        assert_eq!(
                            control.step_instruction().unwrap(),
                            restored.step_instruction().unwrap()
                        );
                        if step >= 8 && control.registers().d[2] == 0x6b {
                            break;
                        }
                    }
                    assert_eq!(control.registers().d[2], 0x6b);
                    for _ in 0..2 {
                        control.step_field().unwrap();
                        restored.step_field().unwrap();
                        assert_eq!(control.timing_snapshot(), restored.timing_snapshot());
                        assert_eq!(control.input_fault(), None);
                        assert!(
                            control.encode_state().unwrap() == restored.encode_state().unwrap()
                        );
                        assert!(
                            control.encode_frontend_state().unwrap()
                                == restored.encode_frontend_state().unwrap()
                        );
                        assert!(control.framebuffer() == restored.framebuffer());
                        let left = control.drain_audio_samples();
                        let right = restored.drain_audio_samples();
                        assert_eq!(left.len(), right.len());
                        assert!(
                            left.iter()
                                .zip(&right)
                                .all(|(a, b)| a.to_bits() == b.to_bits())
                        );
                    }
                }
            }
        }
    }
}
