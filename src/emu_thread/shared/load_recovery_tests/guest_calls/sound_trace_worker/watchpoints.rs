use super::*;
use crate::debug::{
    DebugTarget,
    sound_watchpoints::{SoundWatchBinding, SoundWatchRequest, SoundWatchView},
};
use crate::ui::TraceStreamId;
use zeff_md_core::sound_trace::{SoundAccessKind, SoundWatchRule, SoundWatchSpace};

fn fixture(header: u8) -> EmuBackend {
    let mut words = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
    for value in [0x80, 0x10, 0x90] {
        words.extend([0x13fc, value, 0xc0, 0x11]);
    }
    // Fill the PCM filter history while BUSREQ holds SOUND.
    words.extend([0x303c, 600, 0x51c8, 0xfffe]);
    for (address, value) in [0x3a, 0, 0x10, 0x32, 0, 0x10, 0xdb, 0x12, 0xd3, 0x12, 0x76]
        .into_iter()
        .enumerate()
        .map(|(a, v)| (a as u16, v))
        .chain([(0x1000, 0x5a)])
    {
        words.extend([0x13fc, value, 0xa0, address]);
    }
    words.extend([
        0x33fc, 0x100, 0xa1, 0x1200, 0x33fc, 0, 0xa1, 0x1100, 0x60fe, 0x4e71,
    ]);
    let mut backend = guest_call_backend_with(
        header,
        zeff_md_core::TimingModel::Standard,
        zeff_md_core::ControllerKind::ThreeButton,
        &[(0x200, &words)],
        false,
    );
    backend.debug_suspend();
    backend
}
fn input(frames: usize) -> FrameInput {
    let mut input = guest_call_input(frames);
    input.snapshot.show_instruction_trace = false;
    input.snapshot.debug_target = DebugTarget::Sound;
    input.snapshot.debug_revision = 91;
    input
}
fn view(frame: &FrameResult) -> &SoundWatchView {
    frame.ui_data.md_sound_watch.as_ref().unwrap()
}
fn edit(input: &mut FrameInput, binding: SoundWatchBinding, rule: SoundWatchRule, add: bool) {
    input.debug_actions.target = DebugTarget::Sound;
    input.debug_actions.target_revision = Some(binding.revision);
    input.debug_actions.sound_trace.watch = Some(SoundWatchRequest {
        binding,
        rule,
        add,
        request: TraceStreamId::allocate(),
    });
}
fn time(backend: &EmuBackend) -> u64 {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core().timing_snapshot().now().get()
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_standard_sound_worker_watchpoints_trace_off_stop_after_actual_main_boundary_and_keep_pcm() {
    for header in *b"UJE" {
        let mut worker = EmuThread::spawn(fixture(header), false);
        let mut ordinary = fixture(header);
        let initial = guest_call_deliver(&worker, input(0));
        control_run(&mut ordinary, input(0));
        let memory = SoundWatchRule {
            space: SoundWatchSpace::Memory,
            start: 0x1000,
            end: 0x1000,
            kind: WatchType::ReadWrite,
        };
        let io = SoundWatchRule {
            space: SoundWatchSpace::Io,
            start: 0x12,
            end: 0x12,
            kind: WatchType::ReadWrite,
        };
        let mut arm = input(0);
        edit(&mut arm, view(&initial).binding.unwrap(), memory, true);
        let first = guest_call_deliver(&worker, arm);
        let mut arm = input(0);
        edit(&mut arm, view(&first).binding.unwrap(), io, true);
        arm.debug_actions.sound_trace.enabled = Some(true);
        arm.debug_actions.sound_trace.clear = true;
        arm.debug_actions.trace_enabled = Some(true);
        arm.debug_actions.trace_capacity = Some(10_000);
        arm.snapshot.show_instruction_trace = true;
        arm.snapshot.trace_cursor.target = TraceTarget::Primary;
        let armed = guest_call_deliver(&worker, arm);
        let trace = armed.ui_data.instruction_trace.as_ref().unwrap();
        assert_eq!(trace.target, TraceTarget::Primary);
        assert!(trace.enabled);
        assert_eq!(trace.capacity, 10_000);
        let primary_stream = trace.stream;
        assert_eq!(view(&armed).watches.len(), 2);
        let binding = view(&armed).binding;
        let mut sound = input(0);
        sound.snapshot.show_instruction_trace = true;
        sound.snapshot.trace_cursor.target = TraceTarget::Sound;
        let sound = batch(guest_call_deliver(&worker, sound));
        assert!(sound.enabled && sound.entries.is_empty());
        let mut sound_stream = sound.stream;
        for enabled in [false, true, false] {
            let mut policy = input(0);
            policy.debug_actions.target = DebugTarget::Sound;
            policy.debug_actions.target_revision = Some(91);
            policy.debug_actions.sound_trace.enabled = Some(enabled);
            policy.debug_actions.sound_trace.capacity = Some(0);
            policy.debug_actions.sound_trace.clear = true;
            policy.snapshot.show_instruction_trace = true;
            policy.snapshot.trace_cursor.target = TraceTarget::Sound;
            let frame = guest_call_deliver(&worker, policy);
            assert_eq!(view(&frame).binding, binding);
            assert_eq!(view(&frame).watches, view(&armed).watches);
            assert_eq!(view(&frame).gaps, view(&armed).gaps);
            assert!(view(&frame).watches.iter().all(|watch| watch.hits == 0));
            let trace = batch(frame);
            assert_eq!(trace.target, TraceTarget::Sound);
            assert_eq!(trace.enabled, enabled);
            assert_eq!(trace.capacity, zeff_emu_common::debug::MIN_TRACE_CAPACITY);
            assert!(trace.entries.is_empty());
            assert!(trace.stream > sound_stream);
            sound_stream = trace.stream;
            assert_eq!(capture(&worker), ordinary.encode_state_bytes().unwrap());
        }
        let mut disabled = input(0);
        disabled.snapshot.show_instruction_trace = true;
        disabled.snapshot.trace_cursor.target = TraceTarget::Sound;
        let disabled = guest_call_deliver(&worker, disabled);
        let trace = disabled.ui_data.instruction_trace.as_ref().unwrap();
        assert!(!trace.enabled && trace.entries.is_empty());
        let mut observations = Vec::new();
        let mut heard_at_stop = false;
        for _ in 0..4 {
            let mut run = input(1);
            run.debug_continue = true;
            let actual = guest_call_deliver(&worker, run);
            assert!(actual.runtime_fault.is_none());
            assert_eq!(actual.advanced_frames, 0);
            let hit = view(&actual).hit.unwrap();
            let mut samples = Vec::new();
            while time(&ordinary) < hit.master_tick {
                let mut step = input(0);
                step.debug_step = true;
                let frame = control_run(&mut ordinary, step);
                assert!(frame.runtime_fault.is_none());
                samples.extend(pcm(&frame));
            }
            assert_eq!(time(&ordinary), hit.master_tick);
            let EmuBackend::Md(md) = &ordinary else {
                unreachable!()
            };
            assert_eq!(hit.main_pc, md.core().registers().pc);
            assert!(!hit.faulted);
            assert_eq!(capture(&worker), ordinary.encode_state_bytes().unwrap());
            assert_eq!(pcm(&actual), samples);
            heard_at_stop |= actual.audio_samples.iter().any(|sample| *sample != 0.0);
            observations.push(hit.observed);
            if observations.len() == 1 {
                let mut retained = input(0);
                retained.snapshot.show_instruction_trace = true;
                retained.snapshot.trace_cursor.target = TraceTarget::Primary;
                let retained = batch(guest_call_deliver(&worker, retained));
                assert_eq!(retained.stream, primary_stream);
                assert!(!retained.entries.is_empty());
                let mut clear = input(0);
                clear.debug_actions.target = DebugTarget::Sound;
                clear.debug_actions.target_revision = Some(91);
                clear.debug_actions.trace_enabled = Some(false);
                clear.debug_actions.trace_clear = true;
                clear.debug_actions.sound_trace.enabled = Some(true);
                clear.snapshot.show_instruction_trace = true;
                clear.snapshot.trace_cursor.target = TraceTarget::Primary;
                let cleared = guest_call_deliver(&worker, clear);
                assert_eq!(view(&cleared).hit, Some(hit));
                let cleared = batch(cleared);
                assert_ne!(cleared.stream, primary_stream);
                assert!(!cleared.enabled && cleared.entries.is_empty());
            }
            let mut stop = input(0);
            stop.debug_actions.target = DebugTarget::Sound;
            stop.debug_actions.target_revision = Some(91);
            stop.debug_actions.sound_trace.enabled = Some(false);
            stop.debug_actions.sound_trace.clear = true;
            let inert = guest_call_deliver(&worker, stop);
            assert_eq!(view(&inert).hit, Some(hit));
            assert_eq!(view(&inert).watches, view(&actual).watches);
            assert_eq!(view(&inert).gaps, view(&actual).gaps);
            assert_eq!(view(&inert).binding, view(&actual).binding);
            assert!(inert.audio_samples.is_empty());
            assert_eq!(capture(&worker), ordinary.encode_state_bytes().unwrap());
        }
        assert_eq!(
            observations
                .iter()
                .map(|hit| (
                    hit.pc,
                    hit.cycle,
                    hit.access.space,
                    hit.access.kind,
                    hit.access.address,
                    hit.access.value
                ))
                .collect::<Vec<_>>(),
            [
                (
                    0,
                    13,
                    SoundWatchSpace::Memory,
                    SoundAccessKind::Read,
                    0x1000,
                    0x5a
                ),
                (
                    3,
                    26,
                    SoundWatchSpace::Memory,
                    SoundAccessKind::Write,
                    0x1000,
                    0x5a
                ),
                (
                    6,
                    37,
                    SoundWatchSpace::Io,
                    SoundAccessKind::Read,
                    0x12,
                    0xff
                ),
                (
                    8,
                    48,
                    SoundWatchSpace::Io,
                    SoundAccessKind::Write,
                    0x12,
                    0xff
                ),
            ]
        );
        let final_view = guest_call_deliver(&worker, input(0));
        assert_eq!(
            view(&final_view)
                .watches
                .iter()
                .map(|watch| watch.hits)
                .collect::<Vec<_>>(),
            [2, 2]
        );
        assert!(!view(&final_view).blocked);
        assert!(heard_at_stop);
        let mut resume = input(2);
        resume.debug_continue = true;
        let future = guest_call_deliver(&worker, resume);
        let mut resume = input(2);
        resume.debug_continue = true;
        let expected = control_run(&mut ordinary, resume);
        assert!(future.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert_eq!((future.advanced_frames, expected.advanced_frames), (2, 2));
        assert_eq!(pcm(&future), pcm(&expected));
        assert!(future.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(capture(&worker), ordinary.encode_state_bytes().unwrap());
        assert!(view(&future).hit.is_none());
        assert_eq!(view(&future).watches, view(&final_view).watches);
        worker.shutdown();
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_standard_sound_worker_watch_requests_guard_target_revision_owner_branch_policy_and_reuse() {
    let mut worker = EmuThread::spawn(fixture(b'U'), false);
    let initial = guest_call_deliver(&worker, input(0));
    let original = view(&initial).binding.unwrap();
    let rule = SoundWatchRule {
        space: SoundWatchSpace::Memory,
        start: 0x1000,
        end: 0x1000,
        kind: WatchType::Write,
    };
    let mut arm = input(0);
    edit(&mut arm, original, rule, true);
    let used = arm.debug_actions.sound_trace.watch.unwrap().request;
    let armed = guest_call_deliver(&worker, arm);
    let current = view(&armed).binding.unwrap();
    assert_ne!(original, current);
    let state = capture(&worker);
    for case in 0..9 {
        let mut bad = input(0);
        edit(&mut bad, current, rule, false);
        let request = bad.debug_actions.sound_trace.watch.as_mut().unwrap();
        match case {
            0 => request.binding.owner = TraceStreamId::allocate(),
            1 => request.binding.branch = original.branch,
            2 => request.binding.policy = original.policy,
            3 => request.binding.revision = 90,
            4 => bad.debug_actions.target = DebugTarget::Primary,
            5 => bad.debug_actions.target_revision = Some(90),
            6 => bad.snapshot.debug_target = DebugTarget::Primary,
            7 => request.request = used,
            _ => {
                request.binding.revision = 90;
                bad.debug_actions.target_revision = Some(90);
                bad.snapshot.debug_revision = 90;
            }
        }
        let frame = guest_call_deliver(&worker, bad);
        assert_eq!(capture(&worker), state);
        let frame = match frame.ui_data.md_sound_watch.as_ref() {
            Some(stale) if case == 8 => {
                assert!(stale.rejection.unwrap().contains("stale"));
                assert!(stale.binding.is_none());
                assert_eq!(stale.watches, view(&armed).watches);
                guest_call_deliver(&worker, input(0))
            }
            Some(_) => frame,
            None if case == 6 => guest_call_deliver(&worker, input(0)),
            None => panic!("expected SOUND watch snapshot for case {case}"),
        };
        assert_eq!(view(&frame).watches, view(&armed).watches);
        assert_eq!(view(&frame).binding, Some(current));
    }
    let mut primary = input(0);
    primary.snapshot.debug_target = DebugTarget::Primary;
    primary.snapshot.debug_revision = 92;
    guest_call_deliver(&worker, primary);
    let mut old = input(0);
    edit(&mut old, current, rule, false);
    let stale = guest_call_deliver(&worker, old);
    assert!(view(&stale).binding.is_none());
    assert_eq!(view(&stale).watches.len(), 1);
    let mut fresh = input(0);
    fresh.snapshot.debug_revision = 93;
    let fresh = guest_call_deliver(&worker, fresh);
    let current = view(&fresh).binding.unwrap();
    let mut remove = input(0);
    remove.snapshot.debug_revision = 93;
    edit(&mut remove, current, rule, false);
    let removed = guest_call_deliver(&worker, remove);
    assert!(view(&removed).watches.is_empty());
    assert_ne!(view(&removed).binding, Some(current));
    worker.shutdown();
}
