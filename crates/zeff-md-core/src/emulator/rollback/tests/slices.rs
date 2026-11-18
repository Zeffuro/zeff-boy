use super::*;
use std::num::NonZeroUsize;

fn run_slices(
    owner: &mut MdRollbackSession,
    config: &MdRollbackConfig,
    budgets: &[usize],
) -> (MdRollbackFrame, usize) {
    for turn in 0..1_000_000 {
        let budget = NonZeroUsize::new(budgets[turn % budgets.len()]).unwrap();
        if let Some(frame) = owner.resume_frame(config, budget).unwrap() {
            return (frame, turn);
        }
    }
    panic!("synthetic MD field did not complete");
}

fn retained(owner: &MdRollbackSession) -> MdRetainedPayload {
    let mut into = MdRetainedAllocations::new();
    owner.visit_retained_allocations(&mut into).unwrap();
    into.finish().unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn sliced_fields_match_normal_and_synchronous_rollback_with_varied_budgets() {
    let patterns: [&[usize]; 2] = [&[1], &[1, 7, 64]];
    for (timing, concurrent) in [
        (TimingModel::Standard, false),
        (TimingModel::Standard, true),
        (TimingModel::Precise, true),
    ] {
        for pads in [
            [ControllerKind::ThreeButton; 2],
            [ControllerKind::SixButton; 2],
            [ControllerKind::ThreeButton, ControllerKind::SixButton],
            [ControllerKind::SixButton, ControllerKind::ThreeButton],
        ] {
            let mut baseline = guest(Region::NtscUs, timing, concurrent, false);
            for (port, kind) in [ControllerPort::One, ControllerPort::Two]
                .into_iter()
                .zip(pads)
            {
                baseline.input_mut().set_controller_kind(port, kind);
            }
            let mut normal = baseline.clone();
            let mut synchronous = lease(baseline.clone());
            let config = synchronous.config().clone();
            let mut sliced: Vec<_> = patterns.iter().map(|_| lease(baseline.clone())).collect();
            let mut middle = Vec::new();
            let mut outputs = Vec::new();
            for (index, words) in [[0x0115, 0x082e], [0x04a2, 0x02d1]].into_iter().enumerate() {
                let words =
                    [0, 1].map(|port| words[port] & MdRollbackInputs::word_mask(pads[port]));
                for (port, word) in [ControllerPort::One, ControllerPort::Two]
                    .into_iter()
                    .zip(words)
                {
                    normal
                        .input_mut()
                        .set_host_pad(port, word as u8, (word >> 8) as u8);
                }
                normal.step_field().unwrap();
                let expected = normal.drain_audio_samples();
                assert!(expected.iter().any(|sample| *sample != 0.0));
                let inputs = MdRollbackInputs::new(words, &config).unwrap();
                let sync = synchronous.advance_frame(inputs, &config).unwrap();
                assert_eq!(pcm(&sync.audio), pcm(&expected));
                same(synchronous.core(), &normal);
                for (owner, budgets) in sliced.iter_mut().zip(patterns) {
                    let before = owner.core().clone();
                    let hashes = concurrent.then(|| owner.hashes().unwrap());
                    owner.begin_frame(inputs, &config).unwrap();
                    assert!(owner.has_pending_frame());
                    same(owner.core(), &before);
                    if let Some(hashes) = hashes {
                        assert_eq!(owner.hashes().unwrap(), hashes);
                    }
                    let (frame, yields) = run_slices(owner, &config, budgets);
                    assert!(yields > 0);
                    assert!(!owner.has_pending_frame());
                    assert_eq!(frame.field, sync.field);
                    assert_eq!(frame.master_tick, sync.master_tick);
                    assert_eq!(frame.nominal_duration_ns, sync.nominal_duration_ns);
                    assert_eq!(pcm(&frame.audio), pcm(&expected));
                    assert_eq!(owner.pending_audio_frames(), 0);
                    same(owner.core(), &normal);
                    if concurrent {
                        assert_eq!(owner.hashes().unwrap(), synchronous.hashes().unwrap());
                    }
                    if index == 0 {
                        middle.push(owner.capture().unwrap());
                    }
                }
                outputs.push(pcm(&expected));
            }
            let words = [0x04a2, 0x02d1];
            let words = [0, 1].map(|port| words[port] & MdRollbackInputs::word_mask(pads[port]));
            let inputs = MdRollbackInputs::new(words, &config).unwrap();
            for ((mut owner, budgets), saved) in sliced.into_iter().zip(patterns).zip(middle) {
                owner.restore(&saved, &config).unwrap();
                owner.begin_frame(inputs, &config).unwrap();
                let (replay, _) = run_slices(&mut owner, &config, budgets);
                assert_eq!(pcm(&replay.audio), outputs[1]);
                same(owner.core(), &normal);
                same(&owner.release(), &baseline);
            }
            same(&synchronous.release(), &baseline);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pending_field_observations_abort_and_return_preserve_completed_and_original_pcm() {
    let one = NonZeroUsize::new(1).unwrap();
    let wrong =
        MdRollbackConfig::capture(&guest(Region::Pal, TimingModel::Standard, true, false)).unwrap();
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
            let mut original = guest(Region::NtscUs, timing, true, false);
            for port in [ControllerPort::One, ControllerPort::Two] {
                original.input_mut().set_controller_kind(port, kind);
            }
            original.step_field().unwrap();
            let original_pcm = pcm(&original.clone().drain_audio_samples());
            assert!(original_pcm.iter().any(|sample| *sample != 0));
            let core = Box::new(original.clone());
            let address = &*core as *const Emulator;
            let config = MdRollbackConfig::capture(&core).unwrap();
            let mut owner = match MdRollbackSession::acquire(core, &config) {
                Ok(owner) => owner,
                Err((_, error)) => panic!("baseline rejected: {error:#}"),
            };
            let inputs = MdRollbackInputs::new([0x0101, 0x080a], &config).unwrap();
            assert!(owner.begin_frame(inputs, &config).is_err());
            assert!(!owner.has_pending_frame());
            assert_eq!(pcm(&owner.take_pending_audio().unwrap()), original_pcm);
            let completed = owner.core().clone();
            let saved = owner.capture().unwrap();
            let hashes = owner.hashes().unwrap();
            let before_cost = retained(&owner);
            owner.begin_frame(inputs, &config).unwrap();
            assert!(owner.resume_frame(&config, one).unwrap().is_none());
            assert!(owner.has_pending_frame());
            assert!(owner.capture().is_err());
            assert!(owner.restore(&saved, &config).is_err());
            assert!(owner.take_pending_audio().is_err());
            assert!(owner.begin_frame(inputs, &config).is_err());
            assert!(owner.has_pending_frame());
            same(owner.core(), &completed);
            assert_eq!(owner.hashes().unwrap(), hashes);
            assert_eq!(owner.pending_audio_frames(), 0);
            let pending_cost = retained(&owner);
            assert_eq!(before_cost.count(MdAllocationKind::CoreLayout), 2);
            assert_eq!(pending_cost.count(MdAllocationKind::CoreLayout), 3);
            assert!(
                pending_cost.bytes(MdAllocationKind::WorkRam)
                    > before_cost.bytes(MdAllocationKind::WorkRam)
            );
            assert_eq!(
                pending_cost.count(MdAllocationKind::Rom),
                before_cost.count(MdAllocationKind::Rom)
            );
            assert_eq!(
                pending_cost.bytes(MdAllocationKind::Rom),
                before_cost.bytes(MdAllocationKind::Rom)
            );
            owner.abort_frame();
            owner.abort_frame();
            assert!(!owner.has_pending_frame());
            same(owner.core(), &completed);
            assert_eq!(
                retained(&owner).requested_payload_bytes(),
                before_cost.requested_payload_bytes()
            );
            owner.restore(&saved, &config).unwrap();
            owner.begin_frame(inputs, &config).unwrap();
            assert!(owner.resume_frame(&wrong, one).is_err());
            assert!(!owner.has_pending_frame());
            same(owner.core(), &completed);
            owner.capture().unwrap();
            owner.begin_frame(inputs, &config).unwrap();
            assert!(owner.resume_frame(&config, one).unwrap().is_none());
            owner.retire();
            assert!(!owner.has_pending_frame());
            assert!(owner.capture().is_err());
            let released = owner.release();
            assert_eq!(&*released as *const Emulator, address);
            same(&released, &original);
            assert_eq!(pcm(&released.clone().drain_audio_samples()), original_pcm);
            let address = &*released as *const Emulator;
            let mut owner = match MdRollbackSession::acquire(released, &config) {
                Ok(owner) => owner,
                Err((_, error)) => panic!("released baseline rejected: {error:#}"),
            };
            owner.take_pending_audio().unwrap();
            owner.begin_frame(inputs, &config).unwrap();
            assert!(owner.resume_frame(&config, one).unwrap().is_none());
            let released = owner.release();
            assert_eq!(&*released as *const Emulator, address);
            same(&released, &original);
            assert_eq!(pcm(&released.clone().drain_audio_samples()), original_pcm);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn late_sliced_guest_fault_discards_candidate_and_allows_clean_replay() {
    let one = NonZeroUsize::new(1).unwrap();
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let baseline = guest(Region::NtscUs, timing, true, true);
        let mut owner = lease(baseline.clone());
        let config = owner.config().clone();
        let rejected = MdRollbackInputs::new([1, 0], &config).unwrap();
        owner.begin_frame(rejected, &config).unwrap();
        let mut yields = 0;
        loop {
            match owner.resume_frame(&config, one) {
                Ok(None) => {
                    yields += 1;
                    assert!(yields < 1_000_000);
                }
                Ok(Some(_)) => panic!("faulting guest field completed"),
                Err(_) => break,
            }
        }
        assert!(yields > 0);
        assert!(!owner.has_pending_frame());
        same(owner.core(), &baseline);
        assert_eq!(owner.pending_audio_frames(), 0);
        owner.capture().unwrap();
        let inputs = MdRollbackInputs::new([0; 2], &config).unwrap();
        owner.begin_frame(inputs, &config).unwrap();
        let (frame, _) = run_slices(&mut owner, &config, &[1, 32, 64]);
        let mut normal = baseline.clone();
        let expected = reference(&mut normal, [0xff; 2]);
        assert_eq!(pcm(&frame.audio), pcm(&expected));
        assert!(frame.audio.iter().any(|sample| *sample != 0.0));
        same(owner.core(), &normal);
        same(&owner.release(), &baseline);
    }
}
