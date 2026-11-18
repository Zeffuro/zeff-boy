use super::*;
use crate::hardware::sound::schedule::SoundInterval;
use crate::sound_trace::{NoSoundCapture, SoundAccessKind, SoundWatchRule, SoundWatchSpace};
use zeff_emu_common::debug::WatchType;

pub(super) fn rule(start: u16, end: u16, kind: WatchType) -> SoundWatchRule {
    SoundWatchRule {
        space: SoundWatchSpace::Memory,
        start,
        end,
        kind,
    }
}
pub(super) fn advance<C: SoundCapture>(core: &mut Emulator, capture: &mut C, begin: u64, end: u64) {
    let interval = SoundInterval::new(begin..end, &mut core.bus.audio);
    core.sound_schedule.advance_owned(
        &mut core.sound_cpu,
        &mut core.bus.sound,
        &mut core.bus.cartridge,
        &mut core.bus.ram,
        (interval, capture),
    );
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_actual_read_and_same_value_store_publish_only_at_deadline_without_trace_storage()
 {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = machine(
            region,
            &[0x60fe, 0x4e71],
            &[0x3a, 0, 0x10, 0x32, 0, 0x10, 0x76],
        );
        actual.bus.sound.ram[0x1000] = 0x5a;
        let mut ordinary = actual.clone();
        let mut continuation = SoundTraceContinuation::default();
        let mut output = InstructionTraceStore::new(0);
        continuation
            .edit_watchpoint(&actual, rule(0x1000, 0x1000, WatchType::ReadWrite), true)
            .unwrap();
        continuation
            .edit_watchpoint(&actual, rule(0, 6, WatchType::Read), true)
            .unwrap();
        let mut cursor = actual.bus.now.get();
        for _ in 0..32 {
            if actual.sound_cpu.cycles() >= 26 {
                break;
            }
            let before = actual.clone();
            let count = continuation.watchpoints()[0].hits;
            advance(
                &mut actual,
                &mut continuation.attach(&mut output),
                cursor,
                cursor + 1,
            );
            advance(&mut ordinary, &mut NoSoundCapture, cursor, cursor + 1);
            let pending = actual.sound_schedule.pending.clone().unwrap();
            let deadline = pending.deadline;
            assert!(deadline > cursor + 1);
            assert_eq!(actual.sound_cpu, before.sound_cpu);
            assert_eq!(continuation.watchpoints()[0].hits, count);
            advance(
                &mut actual,
                &mut continuation.attach(&mut output),
                cursor + 1,
                deadline - 1,
            );
            advance(&mut ordinary, &mut NoSoundCapture, cursor + 1, deadline - 1);
            assert_eq!(actual.sound_cpu, before.sound_cpu);
            assert_eq!(continuation.watchpoints()[0].hits, count);
            advance(
                &mut actual,
                &mut continuation.attach(&mut output),
                deadline - 1,
                deadline,
            );
            advance(&mut ordinary, &mut NoSoundCapture, deadline - 1, deadline);
            assert_eq!(actual.sound_cpu, pending.cpu);
            assert_eq!(actual.sound_cpu, ordinary.sound_cpu);
            assert_eq!(actual.bus.sound, ordinary.bus.sound);
            assert_eq!(actual.sound_schedule, ordinary.sound_schedule);
            assert_eq!(actual.bus.audio, ordinary.bus.audio);
            assert_eq!(actual.bus.sound.ram[0x1000], 0x5a);
            cursor = deadline;
        }
        assert_eq!(continuation.watchpoints()[0].hits, 2);
        assert_eq!(continuation.watchpoints()[1].hits, 0);
        let first = continuation.watch_hit().unwrap();
        assert_eq!(
            (
                first.pc,
                first.cycle,
                first.access.kind,
                first.access.address,
                first.access.value
            ),
            (0, 13, SoundAccessKind::Read, 0x1000, 0x5a)
        );
        assert!(!continuation.watch_blocked());
        assert!(output.is_empty() && !output.is_enabled());
        assert_eq!(output.inactive_observation().unwrap().1, 0);
        let mut actual = machine(
            region,
            &[0x60fe, 0x4e71],
            &[0x3a, 0, 0x10, 0x32, 0, 0x10, 0x76],
        );
        actual.bus.sound.ram[0x1000] = 0x5a;
        let mut ordinary = actual.clone();
        let mut continuation = SoundTraceContinuation::default();
        let mut output = InstructionTraceStore::new(0);
        continuation
            .edit_watchpoint(&actual, rule(0x1000, 0x1000, WatchType::ReadWrite), true)
            .unwrap();
        for _ in 0..24 {
            assert_eq!(
                actual.step_instruction_sound_traced(&mut continuation.attach(&mut output)),
                ordinary.step_instruction()
            );
            exact(&actual, &ordinary);
        }
        assert_eq!(continuation.watchpoints()[0].hits, 2);
        assert_eq!(
            continuation.watch_hit().unwrap().access.kind,
            SoundAccessKind::Read
        );
    }
}
