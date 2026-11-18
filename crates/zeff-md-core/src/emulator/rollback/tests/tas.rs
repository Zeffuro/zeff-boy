use super::*;

fn plain() -> Emulator {
    let mut core = guest(Region::NtscUs, TimingModel::Standard, true, false);
    for port in [ControllerPort::One, ControllerPort::Two] {
        core.input_mut()
            .set_controller_kind(port, ControllerKind::ThreeButton);
    }
    core
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tas_seed_and_staged_decision_keep_original_box_and_future_candidate_pcm_exact() {
    let seed = plain();
    let config = MdRollbackConfig::capture(&seed).unwrap();
    let seed_hashes = config.boundary_hashes(&seed).unwrap();
    let mut original = seed.clone();
    original.step_field().unwrap();
    let saved = original.clone();
    let mut control = seed.clone();
    let expected_pcm = reference(&mut control, [0xe7, 0x5d]);
    for commit in [false, true] {
        let original = Box::new(original.clone());
        let address = original.as_ref() as *const Emulator as usize;
        let prepared = MdRollbackSession::prepare_tas_acquisition(&original, &config).unwrap();
        let mut session = match prepared.acquire_original(original) {
            Ok(session) => session,
            Err((_, error)) => panic!("{error:#}"),
        };
        session
            .install_tas_seed(Box::new(seed.clone()), &config, &seed_hashes)
            .unwrap();
        let frame = session
            .advance_frame(
                MdRollbackInputs::new([0x0101, 0x080a], &config).unwrap(),
                &config,
            )
            .unwrap();
        assert_eq!(pcm(&frame.audio), pcm(&expected_pcm));
        let session = match session.prepare_candidate_promotion(&config, &seed_hashes) {
            Err((session, _)) => session,
            Ok(_) => panic!("stale field-zero proof promoted a field-one candidate"),
        };
        same(session.original(), &saved);
        let hashes = session.hashes().unwrap();
        let staged = match session.prepare_candidate_promotion(&config, &hashes) {
            Ok(staged) => staged,
            Err((_, error)) => panic!("{error:#}"),
        };
        assert_eq!(staged.original() as *const Emulator as usize, address);
        same(staged.original(), &saved);
        same(staged.candidate(), &control);
        if commit {
            let mut candidate = staged.finish_candidate();
            same(&candidate, &control);
            let mut expected = control.clone();
            for _ in 0..2 {
                candidate.step_field().unwrap();
                expected.step_field().unwrap();
                let samples = candidate.drain_audio_samples();
                assert!(samples.iter().any(|&sample| sample != 0.0));
                assert_eq!(pcm(&samples), pcm(&expected.drain_audio_samples()));
                same(&candidate, &expected);
            }
        } else {
            let restored = staged.abort_original();
            assert_eq!(restored.as_ref() as *const Emulator as usize, address);
            same(&restored, &saved);
            assert!(
                restored
                    .clone()
                    .drain_audio_samples()
                    .iter()
                    .any(|&sample| sample != 0.0)
            );
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tas_invalid_seed_foreign_snapshot_and_retirement_do_not_change_netplay_release_original() {
    let bound = Box::new(plain());
    let expected = MdRollbackConfig::capture(&bound).unwrap();
    let prepared = MdRollbackSession::prepare_tas_acquisition(&bound, &expected).unwrap();
    let target = Box::new(plain());
    let target_address = target.as_ref() as *const Emulator as usize;
    let returned = match prepared.acquire_original(target) {
        Err((returned, _)) => returned,
        Ok(_) => panic!("prepared acquisition accepted another allocation"),
    };
    assert_eq!(
        returned.as_ref() as *const Emulator as usize,
        target_address
    );
    same(&returned, &bound);
    let mut core = plain();
    core.step_field().unwrap();
    let baseline = core.clone();
    let config = MdRollbackConfig::capture(&core).unwrap();
    let mut session = lease(core);
    let foreign = lease(baseline.clone()).capture().unwrap();
    assert!(session.restore(&foreign, &config).is_err());
    same(session.core(), &baseline);
    let hashes = session.hashes().unwrap();
    assert!(
        session
            .install_tas_seed(Box::new(baseline.clone()), &config, &hashes)
            .is_err()
    );
    same(session.core(), &baseline);
    session.retire();
    let session = match session.prepare_candidate_promotion(&config, &hashes) {
        Err((session, _)) => session,
        Ok(_) => panic!("retired session promoted"),
    };
    let original = session.original() as *const Emulator as usize;
    let returned = session.release();
    assert_eq!(returned.as_ref() as *const Emulator as usize, original);
    same(&returned, &baseline);
}
