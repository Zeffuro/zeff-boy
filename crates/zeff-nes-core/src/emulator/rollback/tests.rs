use super::*;
use crate::hardware::cpu::StatusFlags;

mod fixture;
mod frame_boundary;
mod generic_mappers;
mod large_allocations;
mod mapper_tests;

#[derive(Debug, PartialEq)]
struct Observation {
    audio: Vec<u32>,
    video: Vec<u8>,
    native: Vec<u8>,
    persistent: Option<Vec<u8>>,
    cpu: String,
    bus: StateLoadRollback,
    mapper_runtime: Vec<u8>,
    ppu_transients: (bool, bool),
}

fn observe(core: &Emulator, audio: Vec<f32>) -> Observation {
    Observation {
        audio: audio.into_iter().map(f32::to_bits).collect(),
        video: core.framebuffer().to_vec(),
        native: core.encode_state().unwrap(),
        persistent: core.dump_persistent_data(),
        cpu: format!("{:?}", core.cpu),
        bus: core.bus.capture_state_load_rollback(),
        mapper_runtime: core.encode_rollback_runtime_state(),
        ppu_transients: (
            core.bus.ppu.suppress_vblank_edge,
            core.bus.ppu.rendering_enabled(),
        ),
    }
}

fn input(frame: usize) -> [u8; 2] {
    [
        (frame as u8).wrapping_mul(17),
        (frame as u8).wrapping_mul(29),
    ]
}

fn ordinary_frame(core: &mut Emulator, ports: [u8; 2]) -> Vec<f32> {
    core.set_input_p1_raw(ports[0]);
    core.set_input_p2_raw(ports[1]);
    core.step_frame();
    assert!(core.frame_ready());
    let mut audio = Vec::new();
    core.drain_audio_into_stereo(&mut audio);
    audio
}

fn core(region: u8, mapper: u8) -> Emulator {
    Emulator::new(&fixture::rom(region, mapper), 48_000.0).unwrap()
}

#[test]
fn corrected_regional_trajectories_match_uninterrupted_execution() {
    for region in 0..3 {
        for phase in 1..=8 {
            for depth in 1..=8 {
                let mut control = core(region, 34);
                let mut subject = core(region, 34);
                let session = subject.begin_rollback_session().unwrap();
                for frame in 0..phase {
                    let expected = ordinary_frame(&mut control, input(frame));
                    let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
                    assert_eq!(observe(&subject, actual), observe(&control, expected));
                }
                let snapshot = session.capture(&subject).unwrap();
                assert_eq!(snapshot.frame(), phase as u64);
                for frame in phase..phase + depth {
                    session
                        .advance_frame(&mut subject, input(frame + 17))
                        .unwrap();
                }
                let expected: Vec<_> = (phase..phase + depth + 3)
                    .map(|frame| {
                        let audio = ordinary_frame(&mut control, input(frame));
                        observe(&control, audio)
                    })
                    .collect();
                for _ in 0..3 {
                    session.restore(&mut subject, &snapshot).unwrap();
                    for (offset, reference) in expected.iter().enumerate() {
                        let audio = session
                            .advance_frame(&mut subject, input(phase + offset))
                            .unwrap();
                        assert_eq!(
                            &observe(&subject, audio),
                            reference,
                            "region {region}, phase {phase}, depth {depth}, offset {offset}"
                        );
                    }
                }
                assert!(control.cpu_irq_count() > 0, "fixture must exercise DMC IRQ");
                assert!(control.cpu_nmi_count() > 0, "fixture must exercise NMI");
                assert!(
                    expected
                        .iter()
                        .any(|frame| frame.audio.iter().any(|bits| *bits != 0))
                );
                assert!(
                    control
                        .dump_persistent_data()
                        .unwrap()
                        .iter()
                        .any(|byte| *byte != 0)
                );
            }
        }
    }
}

#[test]
fn fresh_nrom_snapshot_replays_and_does_not_change_native_format() {
    let mut subject = core(0, 0);
    let mut control = core(0, 0);
    let before = subject.encode_state().unwrap();
    let session = subject.begin_rollback_session().unwrap();
    let snapshot = session.capture(&subject).unwrap();
    assert_eq!(snapshot.frame(), 0);
    assert_eq!(snapshot.encoded_native_bytes(), before.len());
    assert!(
        snapshot.retained_bytes() > snapshot.encoded_native_bytes() + subject.framebuffer().len()
    );
    assert_eq!(snapshot.state, before);
    assert_eq!(subject.encode_state().unwrap(), before);
    session.advance_frame(&mut subject, [0xff, 0x42]).unwrap();
    let warmed = session.capture(&subject).unwrap();
    assert!(warmed.retained_bytes() > snapshot.retained_bytes());
    session.restore(&mut subject, &snapshot).unwrap();
    assert_eq!(subject.encode_state().unwrap(), before);
    for frame in 0..4 {
        let expected = ordinary_frame(&mut control, input(frame));
        let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
        assert_eq!(observe(&subject, actual), observe(&control, expected));
    }
}

#[test]
fn corrected_inputs_switch_banks_and_restore_nvram() {
    let mut subject = core(0, 34);
    let session = subject.begin_rollback_session().unwrap();
    session.advance_frame(&mut subject, [0, 0]).unwrap();
    let snapshot = session.capture(&subject).unwrap();
    for _ in 0..3 {
        session.advance_frame(&mut subject, [0x80, 0]).unwrap();
    }
    assert_eq!(subject.cpu_peek(0x6003), 0x32);
    assert_eq!(subject.rom_mapping_token(), 1);
    session.restore(&mut subject, &snapshot).unwrap();
    assert_eq!(subject.rom_mapping_token(), 0);
    for _ in 0..3 {
        session.advance_frame(&mut subject, [0, 0]).unwrap();
    }
    assert_eq!(subject.cpu_peek(0x6003), 0x31);
    assert_eq!(subject.rom_mapping_token(), 0);
}

fn seed_transients(core: &mut Emulator) {
    core.cpu.delay_nmi_poll_once();
    core.cpu.delay_irq_poll_once();
    core.cpu.delay_irq_inhibit_change();
    core.cpu.regs.set_flag(StatusFlags::INTERRUPT, false);
    core.cpu.nmi_pending = true;
    core.cpu.irq_line = true;
    core.bus.ppu.write_mask(0);
}

#[test]
fn sidecars_preserve_cpu_poll_ppu_transients_and_warmed_audio() {
    for region in 0..3 {
        let mut control = core(region, 34);
        let mut subject = core(region, 34);
        let session = subject.begin_rollback_session().unwrap();
        for frame in 0..3 {
            ordinary_frame(&mut control, input(frame));
            session.advance_frame(&mut subject, input(frame)).unwrap();
        }
        seed_transients(&mut control);
        seed_transients(&mut subject);
        let before = observe(&subject, Vec::new());
        let snapshot = session.capture(&subject).unwrap();
        session.advance_frame(&mut subject, [0, 0]).unwrap();
        session.restore(&mut subject, &snapshot).unwrap();
        assert_eq!(observe(&subject, Vec::new()), before);
        for frame in 3..7 {
            let expected = ordinary_frame(&mut control, input(frame));
            let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
            assert_eq!(observe(&subject, actual), observe(&control, expected));
        }
    }
}

#[test]
fn suppressed_vblank_roundtrips_without_losing_the_frame() {
    for region in 0..3 {
        let mut subject = core(region, 34);
        let session = subject.begin_rollback_session().unwrap();
        session.advance_frame(&mut subject, [0, 0]).unwrap();
        subject.bus.ppu.suppress_vblank_edge = true;
        let snapshot = session.capture(&subject).unwrap();
        subject.bus.ppu.suppress_vblank_edge = false;
        session.restore(&mut subject, &snapshot).unwrap();
        assert!(subject.bus.ppu.suppress_vblank_edge);
        session.advance_frame(&mut subject, [0, 0]).unwrap();
        assert!(subject.frame_ready());
        assert_eq!(subject.frame_count(), snapshot.frame() + 1);
        assert!(session.capture(&subject).is_ok());
    }
}

#[test]
fn suspended_execution_is_refused_before_mutation() {
    for region in 0..3 {
        let mut subject = core(region, 34);
        let session = subject.begin_rollback_session().unwrap();
        let snapshot = session.capture(&subject).unwrap();
        subject.cpu.state = crate::hardware::cpu::CpuState::Suspended;
        let before = observe(&subject, Vec::new());
        assert!(session.advance_frame(&mut subject, [0, 0]).is_err());
        assert_eq!(observe(&subject, Vec::new()), before);
        assert!(subject.rollback_frame_boundary);
        assert!(subject.rollback_owner.upgrade().is_some());
        assert!(session.capture(&subject).is_err());
        assert!(session.restore(&mut subject, &snapshot).is_err());
        assert!(subject.begin_rollback_session().is_err());
        subject.cpu.state = crate::hardware::cpu::CpuState::Running;
        session.restore(&mut subject, &snapshot).unwrap();
        assert!(session.capture(&subject).is_ok());
    }
}

#[test]
fn foreign_stale_and_retired_tokens_are_refused_before_mutation() {
    let mut first = core(0, 0);
    let mut second = core(0, 0);
    let first_session = first.begin_rollback_session().unwrap();
    let snapshot = first_session.capture(&first).unwrap();
    let second_session = second.begin_rollback_session().unwrap();
    let before = observe(&second, Vec::new());
    assert!(first_session.capture(&second).is_err());
    assert!(first_session.advance_frame(&mut second, [1, 2]).is_err());
    assert!(second_session.restore(&mut second, &snapshot).is_err());
    assert_eq!(observe(&second, Vec::new()), before);
    assert!(first.begin_rollback_session().is_err());
    drop(first_session);
    assert!(snapshot.owner.upgrade().is_none());
    let replacement = first.begin_rollback_session().unwrap();
    let before = observe(&first, Vec::new());
    assert!(replacement.restore(&mut first, &snapshot).is_err());
    assert_eq!(observe(&first, Vec::new()), before);
}

#[test]
fn failed_native_restore_preserves_lease_and_next_trajectory() {
    let mut subject = core(1, 34);
    let mut control = core(1, 34);
    let session = subject.begin_rollback_session().unwrap();
    for frame in 0..3 {
        ordinary_frame(&mut control, input(frame));
        session.advance_frame(&mut subject, input(frame)).unwrap();
    }
    let mut corrupt = session.capture(&subject).unwrap();
    let mut payload = lz4_flex::decompress_size_prepended(&corrupt.state[12..]).unwrap();
    payload.push(0xa5);
    corrupt.state.truncate(12);
    corrupt
        .state
        .extend(lz4_flex::compress_prepend_size(&payload));
    let before = observe(&subject, Vec::new());
    assert!(session.restore(&mut subject, &corrupt).is_err());
    assert!(subject.load_state(&corrupt.state).is_err());
    assert_eq!(observe(&subject, Vec::new()), before);
    assert!(session.capture(&subject).is_ok());
    for frame in 3..6 {
        let expected = ordinary_frame(&mut control, input(frame));
        let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
        assert_eq!(observe(&subject, actual), observe(&control, expected));
    }
}

#[test]
fn direct_native_decoder_retires_lease_on_success_and_failure() {
    for corrupt in [false, true] {
        let mut subject = core(0, 0);
        let session = subject.begin_rollback_session().unwrap();
        let snapshot = session.capture(&subject).unwrap();
        let mut bytes = subject.encode_state().unwrap();
        if corrupt {
            bytes.truncate(1);
        }
        let result = crate::save_state::decode_state(&mut subject, &bytes);
        assert_eq!(result.is_err(), corrupt);
        let after = observe(&subject, Vec::new());
        assert!(session.capture(&subject).is_err());
        assert!(session.restore(&mut subject, &snapshot).is_err());
        assert_eq!(observe(&subject, Vec::new()), after);
    }
}

#[test]
fn external_mutation_retires_lease_even_when_configuration_is_restored() {
    let mutations: &[fn(&mut Emulator)] = &[
        |core| {
            core.reset();
        },
        |core| {
            let state = core.encode_state().unwrap();
            core.load_state(&state).unwrap();
        },
        |core| {
            core.set_sample_rate(44_100);
            core.set_sample_rate(48_000);
        },
        |core| {
            core.set_apu_channel_mutes([true; 5]);
            core.set_apu_channel_mutes([false; 5]);
        },
        |core| {
            core.set_instruction_trace_enabled(true);
            core.set_instruction_trace_enabled(false);
        },
        |core| {
            core.add_breakpoint(0x8123);
            core.remove_breakpoint(0x8123);
        },
        |core| {
            core.cpu_write(0x20, 9);
        },
        |core| {
            core.bus_mut().ram[1] = 3;
        },
        |core| {
            core.step_instruction();
        },
        |core| {
            core.step_frame();
            core.drain_audio_samples();
        },
        |core| {
            core.set_input_p1_raw(0);
        },
        |core| {
            core.clear_frame_ready();
        },
    ];
    for mutate in mutations {
        let mut subject = core(0, 0);
        let session = subject.begin_rollback_session().unwrap();
        let snapshot = session.capture(&subject).unwrap();
        mutate(&mut subject);
        let before = observe(&subject, Vec::new());
        assert!(session.capture(&subject).is_err());
        assert!(session.restore(&mut subject, &snapshot).is_err());
        assert!(session.advance_frame(&mut subject, [0, 0]).is_err());
        assert_eq!(observe(&subject, Vec::new()), before);
    }
}

#[test]
fn admission_rejects_configuration_and_incomplete_boundaries() {
    let rejected: &[fn(&mut Emulator)] = &[
        |core| core.set_sample_rate(44_100),
        |core| core.set_apu_sample_generation_enabled(false),
        |core| core.set_apu_channel_mutes([true; 5]),
        |core| core.set_opcode_log_enabled(true),
        |core| core.set_instruction_trace_enabled(true),
        |core| core.add_breakpoint(0x8000),
        |core| core.set_zapper_state(true, false, false, None),
        |core| {
            core.step_instruction();
        },
        |core| {
            core.step_frame();
        },
    ];
    for configure in rejected {
        let mut subject = core(0, 0);
        configure(&mut subject);
        let before = observe(&subject, Vec::new());
        assert!(subject.begin_rollback_session().is_err());
        assert_eq!(observe(&subject, Vec::new()), before);
    }
    let mut vs = fixture::rom(0, 0);
    vs[7] |= 1;
    assert!(
        Emulator::new(&vs, 48_000.0)
            .unwrap()
            .begin_rollback_session()
            .is_err()
    );
    let mut unsupported = fixture::rom(0, 0);
    unsupported[6] = 0x30;
    unsupported[7] = 0x60;
    assert!(
        Emulator::new(&unsupported, 48_000.0)
            .unwrap()
            .begin_rollback_session()
            .is_err()
    );
    let mut traced = Emulator::new_with_audio_trace(&fixture::rom(0, 0), 48_000.0, 128).unwrap();
    assert!(traced.begin_rollback_session().is_err());
    let mut completed = core(0, 0);
    ordinary_frame(&mut completed, [0, 0]);
    assert!(completed.begin_rollback_session().is_ok());
}
