use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn stale_foreign_clone_decode_and_settings_preserve_owner() {
    let mut baseline = guest(Region::NtscUs, TimingModel::Standard, true, false);
    baseline.step_field().unwrap();
    assert!(
        baseline
            .clone()
            .drain_audio_samples()
            .iter()
            .any(|&sample| sample != 0.0)
    );
    let mut owner = lease(baseline.clone());
    let config = owner.config().clone();
    let saved = owner.capture().unwrap();
    let mut foreign = lease(baseline.clone());
    assert!(foreign.restore(&saved, &config).is_err());
    same(foreign.core(), &baseline);
    foreign.capture().unwrap();
    let mut detached = owner.core().clone();
    assert!(detached.decode_state(b"invalid").is_err());
    same(&detached, &baseline);
    detached
        .decode_frontend_state(&baseline.encode_frontend_state().unwrap())
        .unwrap();
    let clone_before = detached.clone();
    let mut clone_owner = lease(detached);
    assert!(clone_owner.restore(&saved, &config).is_err());
    same(clone_owner.core(), &clone_before);
    clone_owner.capture().unwrap();
    owner.restore(&saved, &config).unwrap();
    let wrong =
        MdRollbackConfig::capture(&guest(Region::Pal, TimingModel::Standard, true, false)).unwrap();
    assert_ne!(wrong.identity_bytes(), config.identity_bytes());
    assert!(owner.restore(&saved, &wrong).is_err());
    assert!(
        owner
            .advance_frame(MdRollbackInputs::new([0; 2], &config).unwrap(), &wrong)
            .is_err()
    );
    same(owner.core(), &baseline);
    owner.capture().unwrap();
    let released = owner.release();
    let mut replacement = lease(*released);
    assert!(replacement.restore(&saved, &config).is_err());
    replacement.capture().unwrap();
    let own_saved = replacement.capture().unwrap();
    replacement.retire();
    assert!(replacement.capture().is_err());
    assert!(replacement.restore(&own_saved, &config).is_err());
    same(&replacement.release(), &baseline);
    for invalid in [[0x1000, 0], [0, 0x1000], [4, 0], [0, 4]] {
        assert!(MdRollbackInputs::new(invalid, &config).is_err());
    }
    let mut changed = baseline.clone();
    changed.set_sample_rate(44_100);
    let changed_before = changed.clone();
    let changed = Box::new(changed);
    let original_address = &*changed as *const Emulator;
    let Err((returned, _)) = MdRollbackSession::acquire(changed, &config) else {
        panic!("changed settings admitted");
    };
    assert_eq!(&*returned as *const Emulator, original_address);
    same(&returned, &changed_before);
    let Err((returned, _)) = MdRollbackSession::acquire(Box::new(baseline.clone()), &wrong) else {
        panic!("foreign configuration admitted");
    };
    same(&returned, &baseline);

    let mut traced = guest(Region::NtscUs, TimingModel::Standard, true, false);
    traced.reset_and_begin_audio_trace(64).unwrap();
    traced.step_field().unwrap();
    assert!(traced.bus.audio.trace.is_enabled());
    traced.apply_ram_cheats(&[zeff_emu_common::cheats::CheatPatch::WideRamWrite {
        address: 0xff0040,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xa5),
    }]);
    assert!(!traced.bus.audio.trace.is_enabled());
    assert!(traced.bus.audio.trace.is_active());
    assert!(MdRollbackConfig::capture(&traced).is_err());
    let queued = pcm(&traced.clone().drain_audio_samples());
    assert!(queued.iter().any(|&sample| sample != 0));
    let before = traced.clone();
    let traced = Box::new(traced);
    let address = &*traced as *const Emulator;
    let Err((mut returned, _)) = MdRollbackSession::acquire(traced, &config) else {
        panic!("retained invalidated trace admitted");
    };
    assert_eq!(&*returned as *const Emulator, address);
    same(&returned, &before);
    assert_eq!(returned.bus.audio.trace, before.bus.audio.trace);
    assert_eq!(pcm(&returned.clone().drain_audio_samples()), queued);
    let finished = returned.finish_audio_trace().unwrap();
    assert_eq!(
        finished.trace.invalidated,
        Some(zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation)
    );
    assert!(!returned.bus.audio.trace.is_active());
    assert_ne!(
        returned.bus.audio.trace,
        zeff_emu_common::audio_trace::MdAudioTraceRecorder::default()
    );
    let before = returned.clone();
    let owner = match MdRollbackSession::acquire(returned, &config) {
        Ok(owner) => owner,
        Err((_, error)) => panic!("finished trace rejected: {error:#}"),
    };
    let returned = owner.release();
    assert_eq!(&*returned as *const Emulator, address);
    same(&returned, &before);
    assert_eq!(returned.bus.audio.trace, before.bus.audio.trace);
    assert_eq!(pcm(&returned.clone().drain_audio_samples()), queued);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_candidate_bus_failure_keeps_runtime_and_release_exact() {
    let baseline = guest(Region::NtscUs, TimingModel::Standard, true, true);
    let mut owner = lease(baseline.clone());
    let config = owner.config().clone();
    let neutral = MdRollbackInputs::new([0; 2], &config).unwrap();
    let first = owner.advance_frame(neutral, &config).unwrap();
    assert!(first.audio.iter().any(|&sample| sample != 0.0));
    let before = owner.core().clone();
    let saved = owner.capture().unwrap();
    let rejected = MdRollbackInputs::new([1, 0], &config).unwrap();
    assert!(owner.advance_frame(rejected, &config).is_err());
    same(owner.core(), &before);
    assert_eq!(owner.pending_audio_frames(), 0);
    owner.capture().unwrap();
    owner.restore(&saved, &config).unwrap();
    let mut control = before;
    let expected = reference(&mut control, [0xff; 2]);
    let output = owner.advance_frame(neutral, &config).unwrap();
    assert_eq!(pcm(&output.audio), pcm(&expected));
    same(owner.core(), &control);
    same(&owner.release(), &baseline);
}
