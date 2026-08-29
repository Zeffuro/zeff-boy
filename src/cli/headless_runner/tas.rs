use crate::cli::types::HeadlessOptions;

pub(super) fn validate_tas_system(opts: &HeadlessOptions, system: &str) -> anyhow::Result<()> {
    if let Some(script) = &opts.tas_script {
        anyhow::ensure!(
            script.system == system,
            "TAS script targets system {:?}, but loaded media is {system:?}",
            script.system
        );
        anyhow::ensure!(
            opts.break_at.is_none() && !opts.expect_test_pass && !opts.break_on_gba_bad_state,
            "--tas-script cannot be combined with breakpoint or early test-exit options"
        );
        anyhow::ensure!(
            opts.ws_link_peer_path.is_none(),
            "--tas-script does not support paired WonderSwan link runs"
        );
    }
    Ok(())
}

pub(super) fn check_tas_assertions(
    opts: &HeadlessOptions,
    frame: u64,
    pc: u32,
    framebuffer: &[u8],
    encode_state: impl FnOnce() -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<()> {
    let Some(script) = &opts.tas_script else {
        return Ok(());
    };
    let start = script
        .assertions
        .partition_point(|assertion| assertion.frame < frame);
    let end =
        script.assertions[start..].partition_point(|assertion| assertion.frame == frame) + start;
    let assertions = &script.assertions[start..end];
    if assertions.is_empty() {
        return Ok(());
    }

    let framebuffer_hash = assertions
        .iter()
        .any(|assertion| assertion.framebuffer_sha256.is_some())
        .then(|| zeff_firmware::sha256_bytes(framebuffer));
    let state_hash = if assertions
        .iter()
        .any(|assertion| assertion.state_sha256.is_some())
    {
        Some(zeff_firmware::sha256_bytes(&encode_state()?))
    } else {
        None
    };

    for assertion in assertions {
        if let Some(expected) = assertion.pc {
            anyhow::ensure!(
                pc == expected,
                "TAS assertion {:?} failed at frame {frame}: expected pc=0x{expected:X}, got 0x{pc:X}",
                assertion.name
            );
        }
        if let Some(expected) = assertion.framebuffer_sha256 {
            let actual = framebuffer_hash.expect("framebuffer hash was requested");
            anyhow::ensure!(
                actual == expected,
                "TAS assertion {:?} failed at frame {frame}: expected framebuffer-sha256={}, got {}",
                assertion.name,
                const_hex::encode(expected),
                const_hex::encode(actual)
            );
        }
        if let Some(expected) = assertion.state_sha256 {
            let actual = state_hash.expect("state hash was requested");
            anyhow::ensure!(
                actual == expected,
                "TAS assertion {:?} failed at frame {frame}: expected state-sha256={}, got {}",
                assertion.name,
                const_hex::encode(expected),
                const_hex::encode(actual)
            );
        }
        println!(
            "[headless] tas-assert name={} frame={} status=ok",
            assertion.name, frame
        );
    }
    Ok(())
}

pub(super) fn ensure_tas_completed(opts: &HeadlessOptions, frames_run: u64) -> anyhow::Result<()> {
    if opts.tas_script.is_some() {
        anyhow::ensure!(
            frames_run == opts.max_frames,
            "TAS script ended at frame {frames_run} before declared frame {}",
            opts.max_frames
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::types::{HeadlessTasAssertion, HeadlessTasScript};

    use super::*;

    fn options(assertion: HeadlessTasAssertion) -> HeadlessOptions {
        HeadlessOptions {
            max_frames: 4,
            tas_script: Some(HeadlessTasScript {
                system: "gb".to_owned(),
                assertions: vec![assertion],
            }),
            ..HeadlessOptions::default()
        }
    }

    #[test]
    fn validates_post_frame_hashes_and_pc() {
        let framebuffer = [1, 2, 3, 4];
        let state = [5, 6, 7, 8];
        let opts = options(HeadlessTasAssertion {
            name: "checkpoint".to_owned(),
            frame: 4,
            pc: Some(0x1234),
            state_sha256: Some(zeff_firmware::sha256_bytes(&state)),
            framebuffer_sha256: Some(zeff_firmware::sha256_bytes(&framebuffer)),
        });

        check_tas_assertions(&opts, 4, 0x1234, &framebuffer, || Ok(state.to_vec())).unwrap();
        let err = check_tas_assertions(&opts, 4, 0x1235, &framebuffer, || Ok(state.to_vec()))
            .unwrap_err();
        assert!(err.to_string().contains("expected pc=0x1234, got 0x1235"));
        let hash_opts = options(HeadlessTasAssertion {
            name: "checkpoint".to_owned(),
            frame: 4,
            pc: None,
            state_sha256: None,
            framebuffer_sha256: Some([0; 32]),
        });
        let err =
            check_tas_assertions(&hash_opts, 4, 0, &framebuffer, || Ok(Vec::new())).unwrap_err();
        assert!(
            err.to_string()
                .contains(&const_hex::encode(zeff_firmware::sha256_bytes(
                    &framebuffer
                )))
        );
        assert!(ensure_tas_completed(&opts, 3).is_err());
        ensure_tas_completed(&opts, 4).unwrap();
    }

    #[test]
    fn rejects_loaded_system_mismatch() {
        let opts = options(HeadlessTasAssertion {
            name: "checkpoint".to_owned(),
            frame: 4,
            pc: Some(0),
            state_sha256: None,
            framebuffer_sha256: None,
        });

        assert!(validate_tas_system(&opts, "gba").is_err());
        validate_tas_system(&opts, "gb").unwrap();
    }
}
