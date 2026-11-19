use super::*;
use crate::hardware::sound::schedule::PendingSound;
use zeff_z80::{Cpu, ResetState};

#[test]
fn restored_last_boundary_completes_the_same_single_instruction() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for interlace in [false, true] {
                let mut actual = fixture(region, concurrent, &[0x60fe, 0x4e71]);
                configure(&mut actual, true, interlace);
                reference_field(&mut actual).0.unwrap();
                let field = actual.pacing_field_count();
                let mut planning = actual.clone();
                let (result, boundaries) = reference_field(&mut planning);
                result.unwrap();
                assert!(boundaries.count > 1);
                for _ in 1..boundaries.count {
                    actual.step_instruction().unwrap();
                    assert_eq!(actual.pacing_field_count(), field);
                }
                let state = actual.encode_state().unwrap();
                actual.decode_state(&state).unwrap();
                let before = actual.bus.now;
                let mut expected = actual.clone();
                let (result, boundaries) = compare_field(&mut actual, &mut expected);
                result.unwrap();
                assert_eq!(boundaries.count, 1);
                assert!(actual.bus.now > before);
                assert_ne!(actual.pacing_field_count(), field);
            }
        }
    }
}

#[test]
fn failed_status_refresh_flushes_admitted_audio_and_preserves_first_fault() {
    let mut actual = fixture(Region::NtscUs, true, &[0x60fe, 0x4e71]);
    actual.reset_and_begin_audio_trace(16).unwrap();
    actual.sound_cpu = Cpu::new_with_reset(ResetState::new(0x4000, 0x1ffe));
    actual.bus.sound.set_reset(false);
    actual.sound_schedule.pending = Some(
        PendingSound::prepare(
            &actual.sound_cpu,
            &actual.bus.sound,
            &actual.bus.cartridge,
            actual.bus.now.get(),
        )
        .unwrap(),
    );
    let pending = actual.sound_schedule.pending.as_ref().unwrap();
    assert_eq!(pending.instruction.opcode, 0);
    assert!(pending.deadline > actual.bus.now.get());
    let now = actual.bus.now.get();
    actual.bus.audio.write_opn2(now, 0, 0x2a);
    actual.bus.audio.write_opn2(now, 1, 0x90);
    let mut expected = actual.clone();
    let (result, boundaries) = compare_field(&mut actual, &mut expected);
    assert_eq!(
        result,
        Err(MachineFault::Sound(SoundFault::UnstableOpn2Read {
            pc: 0x4000,
        }))
    );
    assert_eq!(boundaries.count, 1);
    assert!(actual.bus.now.get() > now);
    assert_eq!(
        actual
            .clone()
            .finish_audio_trace()
            .unwrap()
            .trace
            .invalidated,
        Some(AudioTraceInvalidation::ExecutionFault)
    );
    let stopped_at = actual.bus.now;
    compare_field(&mut actual, &mut expected).0.unwrap_err();
    assert_eq!(actual.bus.now, stopped_at);
}
