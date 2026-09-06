use super::{
    CALLBACK_STATE, CallbackCounts, CallbackState, ContinuationCore, CoreOptionValue, JoypadInput,
    RETRO_DEVICE_JOYPAD, RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
    RETRO_ENVIRONMENT_GET_LANGUAGE, RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
    RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2, RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL,
    RETRO_NUM_CORE_OPTION_VALUES_MAX, RETRO_PIXEL_FORMAT_RGB565, RetroCoreOptionV2Definition,
    RetroCoreOptionValue, RetroCoreOptionsV2, RetroCoreOptionsV2Intl, RetroLogCallback,
    RetroVariable, SaveRamSnapshot, audio_batch, audio_sample, capture_rgb24,
    check_state_and_continuation, copy_save_ram, environment, get_variable, input_poll,
    input_state, percentile, repeated_callback_hashes_match, serialize_rejects_undersized_buffer,
    validate_callback_buffers, video_refresh,
};
use sha2::Digest;
use std::ffi::CString;

struct SyntheticContinuationCore {
    counter: u8,
    save_ram: Vec<u8>,
    checkpoint_save_ram: u8,
    reject_restore: bool,
    restore_save_ram: bool,
    hidden_restore_bias: u8,
    reject_serialize_after_restore: bool,
    restored: bool,
    uninterrupted_inputs: Vec<i16>,
    restored_inputs: Vec<i16>,
}

impl SyntheticContinuationCore {
    fn exact() -> Self {
        Self {
            counter: 3,
            save_ram: vec![7],
            checkpoint_save_ram: 7,
            reject_restore: false,
            restore_save_ram: true,
            hidden_restore_bias: 0,
            reject_serialize_after_restore: false,
            restored: false,
            uninterrupted_inputs: Vec::new(),
            restored_inputs: Vec::new(),
        }
    }
}

impl ContinuationCore for SyntheticContinuationCore {
    fn run_frame(&mut self) {
        unsafe { input_poll() };
        let input = unsafe { input_state(0, RETRO_DEVICE_JOYPAD, 0, 8) };
        if self.restored {
            self.restored_inputs.push(input);
        } else {
            self.uninterrupted_inputs.push(input);
        }
        self.counter = self
            .counter
            .wrapping_add(1)
            .wrapping_add(input as u8)
            .wrapping_add(self.hidden_restore_bias * u8::from(self.restored));
        self.save_ram[0] = self.save_ram[0].wrapping_add(self.counter);
        let pixel = u16::from(self.counter).to_ne_bytes();
        unsafe {
            video_refresh(pixel.as_ptr().cast(), 1, 1, 2);
            audio_sample(i16::from(self.counter), input);
        }
    }

    fn serialize(&mut self, state: &mut [u8]) -> bool {
        if self.restored && self.reject_serialize_after_restore {
            return false;
        }
        let Some(output) = state.first_mut() else {
            return false;
        };
        *output = self.counter;
        true
    }

    fn unserialize(&mut self, state: &[u8]) -> bool {
        if self.reject_restore {
            return false;
        }
        let Some(counter) = state.first() else {
            return false;
        };
        self.counter = *counter;
        if self.restore_save_ram {
            self.save_ram[0] = self.checkpoint_save_ram;
        }
        self.restored = true;
        true
    }

    fn snapshot_save_ram(&mut self) -> anyhow::Result<SaveRamSnapshot> {
        Ok(SaveRamSnapshot {
            hash: Some(sha2::Sha256::digest(&self.save_ram).into()),
            bytes: self.save_ram.clone(),
            nonnull: true,
        })
    }
}

#[test]
fn disabled_continuation_restores_and_roundtrips_without_advancing() {
    let mut core = SyntheticContinuationCore::exact();

    let result = check_state_and_continuation(&mut core, &[3], 10, 0).unwrap();

    assert!(result.restore_accepted);
    assert!(result.reserialize_succeeded);
    assert!(result.roundtrip);
    assert!(result.continuation.is_none());
    assert!(core.uninterrupted_inputs.is_empty());
    assert!(core.restored_inputs.is_empty());
}

fn install_continuation_callback_state() -> sha2::Sha256 {
    let mut measured_video = sha2::Sha256::new();
    measured_video.update(b"measured video");
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        frame_index: 9,
        inputs: vec![
            JoypadInput {
                frame: 10,
                port: 0,
                mask: 0x0100,
            },
            JoypadInput {
                frame: 12,
                port: 0,
                mask: 0,
            },
        ],
        counts: CallbackCounts {
            video_calls: 17,
            audio_frames: 23,
            ..CallbackCounts::default()
        },
        video_hasher: measured_video.clone(),
        frame_audio: Some(vec![Default::default(); 10]),
        capture_frame: Some(4),
        capture_audio_s16le: true,
        blackhole_output: true,
        ..CallbackState::default()
    });
    measured_video
}

#[test]
fn continuation_proof_replays_inputs_and_isolates_each_witness() {
    let measured_video = install_continuation_callback_state();
    let mut core = SyntheticContinuationCore::exact();

    let result = check_state_and_continuation(&mut core, &[3], 10, 3).unwrap();

    assert!(result.restore_accepted);
    assert!(result.reserialize_succeeded);
    assert!(result.roundtrip);
    assert_eq!(
        result.save_ram_post_roundtrip.hash,
        Some(sha2::Sha256::digest([7]).into())
    );
    let proof = result.continuation.unwrap();
    assert!(proof.matches);
    assert_eq!(proof.callback_counts_match, Some(true));
    assert_eq!(proof.video_match, Some(true));
    assert_eq!(proof.audio_match, Some(true));
    assert_eq!(proof.state_match, Some(true));
    assert_eq!(proof.save_ram_match, Some(true));
    assert_eq!(proof.uninterrupted.callbacks.video_calls, 3);
    assert_eq!(proof.uninterrupted.callbacks.audio_sample_calls, 3);
    assert_eq!(proof.restored.unwrap().callbacks.video_calls, 3);
    assert_eq!(core.uninterrupted_inputs, [1, 1, 0]);
    assert_eq!(core.restored_inputs, core.uninterrupted_inputs);

    let callback_state = CALLBACK_STATE.lock().unwrap().take().unwrap();
    assert_eq!(callback_state.frame_index, 9);
    assert_eq!(callback_state.counts.video_calls, 17);
    assert_eq!(callback_state.counts.audio_frames, 23);
    assert_eq!(
        callback_state.video_hasher.finalize(),
        measured_video.finalize()
    );
    assert_eq!(callback_state.frame_audio.unwrap().len(), 10);
    assert_eq!(callback_state.capture_frame, Some(4));
    assert!(callback_state.capture_audio_s16le);
    assert!(callback_state.blackhole_output);
}

#[test]
fn continuation_catches_hidden_restore_divergence_after_exact_roundtrip() {
    install_continuation_callback_state();
    let mut core = SyntheticContinuationCore {
        hidden_restore_bias: 1,
        ..SyntheticContinuationCore::exact()
    };

    let result = check_state_and_continuation(&mut core, &[3], 10, 3).unwrap();

    assert!(result.restore_accepted);
    assert!(result.reserialize_succeeded);
    assert!(result.roundtrip);
    let proof = result.continuation.unwrap();
    assert!(!proof.matches);
    assert_eq!(proof.callback_counts_match, Some(true));
    assert_eq!(proof.video_match, Some(false));
    assert_eq!(proof.audio_match, Some(false));
    assert_eq!(proof.state_match, Some(false));
    assert_eq!(proof.save_ram_match, Some(false));
    CALLBACK_STATE.lock().unwrap().take();
}

#[test]
fn continuation_restore_rejection_is_reported_without_a_restored_witness() {
    install_continuation_callback_state();
    let mut core = SyntheticContinuationCore {
        reject_restore: true,
        ..SyntheticContinuationCore::exact()
    };

    let result = check_state_and_continuation(&mut core, &[3], 10, 3).unwrap();

    assert!(!result.restore_accepted);
    assert!(!result.reserialize_succeeded);
    assert!(!result.roundtrip);
    let proof = result.continuation.unwrap();
    assert!(!proof.matches);
    assert!(proof.restored.is_none());
    assert_eq!(proof.video_match, None);
    assert_eq!(proof.audio_match, None);
    assert_eq!(proof.state_match, None);
    assert_eq!(proof.save_ram_match, None);
    CALLBACK_STATE.lock().unwrap().take();
}

#[test]
fn continuation_distinguishes_post_restore_serialization_failure() {
    install_continuation_callback_state();
    let mut core = SyntheticContinuationCore {
        reject_serialize_after_restore: true,
        ..SyntheticContinuationCore::exact()
    };

    let result = check_state_and_continuation(&mut core, &[3], 10, 3).unwrap();

    assert!(result.restore_accepted);
    assert!(!result.reserialize_succeeded);
    assert!(!result.roundtrip);
    let proof = result.continuation.unwrap();
    assert!(!proof.matches);
    let restored = proof.restored.unwrap();
    assert!(!restored.state_serialized);
    assert!(restored.state_hash.is_none());
    assert_eq!(proof.state_match, Some(false));
    CALLBACK_STATE.lock().unwrap().take();
}

#[test]
fn continuation_does_not_repair_sram_that_the_core_failed_to_restore() {
    install_continuation_callback_state();
    let mut core = SyntheticContinuationCore {
        restore_save_ram: false,
        ..SyntheticContinuationCore::exact()
    };

    let result = check_state_and_continuation(&mut core, &[3], 10, 3).unwrap();

    assert!(result.restore_accepted);
    assert!(result.reserialize_succeeded);
    assert!(result.roundtrip);
    assert_ne!(
        result.save_ram_post_roundtrip.hash,
        Some(sha2::Sha256::digest([7]).into())
    );
    let proof = result.continuation.unwrap();
    assert_eq!(proof.video_match, Some(true));
    assert_eq!(proof.audio_match, Some(true));
    assert_eq!(proof.state_match, Some(true));
    assert_eq!(proof.save_ram_match, Some(false));
    assert!(!proof.matches);
    CALLBACK_STATE.lock().unwrap().take();
}

#[test]
fn percentile_uses_a_measured_sample() {
    assert_eq!(percentile(&[10.0, 20.0, 30.0, 40.0, 50.0], 0.5), 30.0);
    assert_eq!(percentile(&[10.0, 20.0, 30.0, 40.0, 50.0], 0.95), 50.0);
}

#[test]
fn disabled_callback_hashes_are_not_evaluated_for_repeats() {
    assert_eq!(
        repeated_callback_hashes_match(false, [None::<[u8; 32]>, None].into_iter()),
        None
    );
    assert_eq!(
        repeated_callback_hashes_match(true, [Some([1; 32]), Some([1; 32])].into_iter()),
        Some(true)
    );
    assert_eq!(
        repeated_callback_hashes_match(true, [Some([1; 32]), Some([2; 32])].into_iter()),
        Some(false)
    );
}

#[test]
fn unavailable_core_option_clears_the_frontend_value_pointer() {
    let key = CString::new("missing").unwrap();
    let stale_value = CString::new("stale").unwrap();
    let mut variable = RetroVariable {
        key: key.as_ptr(),
        value: stale_value.as_ptr(),
    };

    assert!(!get_variable(
        (&mut variable as *mut RetroVariable).cast(),
        &mut CallbackState::default()
    ));
    assert!(variable.value.is_null());
}

#[test]
fn captured_rgb565_frame_converts_visible_pixels_without_pitch_padding() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xF800_u16.to_ne_bytes());
    bytes.extend_from_slice(&0x07E0_u16.to_ne_bytes());
    bytes.extend_from_slice(&[0xAA, 0xBB]);

    let frame = capture_rgb24(&bytes, 2, 1, 6, RETRO_PIXEL_FORMAT_RGB565).unwrap();

    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.rgb24, [0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00]);
}

#[test]
fn captured_frame_rejects_a_bounded_output_overflow_before_reading_pixels() {
    let error = capture_rgb24(&[], 10_000, 10_000, 0, RETRO_PIXEL_FORMAT_RGB565).unwrap_err();

    assert!(error.to_string().contains("captured RGB frame exceeds"));
}

#[test]
fn audio_batch_records_frame_local_telemetry() {
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        frame_index: 1,
        frame_audio: Some(vec![Default::default(), Default::default()]),
        ..CallbackState::default()
    });
    let samples = [1_i16, -2, 3, -4];

    assert_eq!(unsafe { audio_batch(samples.as_ptr(), 2) }, 2);

    let state = CALLBACK_STATE.lock().unwrap().take().unwrap();
    let frame = &state.frame_audio.unwrap()[1];
    assert_eq!(frame.sample_calls, 0);
    assert_eq!(frame.batch_calls, 1);
    assert_eq!(frame.frames, 2);
    assert_eq!(frame.bytes, 8);
    let mut expected = sha2::Sha256::new();
    for sample in samples {
        expected.update(sample.to_le_bytes());
    }
    assert_eq!(frame.hasher.clone().finalize(), expected.finalize());
}

#[test]
fn blackhole_callbacks_validate_and_count_without_hashing_payloads() {
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        blackhole_output: true,
        ..CallbackState::default()
    });
    let pixels = [1_u8, 2, 3, 4, 5, 6, 7, 8];
    let samples = [1_i16, -2, 3, -4];

    unsafe {
        video_refresh(pixels.as_ptr().cast(), 2, 2, 4);
        audio_sample(5, -6);
        assert_eq!(audio_batch(samples.as_ptr(), 2), 2);
    }

    let state = CALLBACK_STATE.lock().unwrap().take().unwrap();
    validate_callback_buffers(&state).unwrap();
    assert_eq!(state.counts.video_calls, 1);
    assert_eq!(state.counts.video_bytes, 8);
    assert_eq!(state.counts.visible_video_bytes, 8);
    assert_eq!(state.counts.audio_sample_calls, 1);
    assert_eq!(state.counts.audio_batch_calls, 1);
    assert_eq!(state.counts.audio_frames, 3);
    assert_eq!(state.counts.audio_bytes, 12);
    assert_eq!(
        state.video_hasher.finalize(),
        sha2::Sha256::new().finalize()
    );
    assert_eq!(
        state.audio_hasher.finalize(),
        sha2::Sha256::new().finalize()
    );
}

#[test]
fn blackhole_video_still_rejects_an_invalid_pitch() {
    let pixels = [0_u8; 8];
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        blackhole_output: true,
        ..CallbackState::default()
    });

    unsafe { video_refresh(pixels.as_ptr().cast(), 2, 2, 3) };

    let state = CALLBACK_STATE.lock().unwrap().take().unwrap();
    assert!(validate_callback_buffers(&state).is_err());
}

#[test]
fn undersized_serialize_probe_keeps_an_ignoring_core_in_bounds() {
    unsafe extern "C" fn ignores_size(buffer: *mut std::ffi::c_void, _size: usize) -> bool {
        unsafe { std::ptr::write_bytes(buffer, 0xA5, 4) };
        true
    }

    assert!(!unsafe { serialize_rejects_undersized_buffer(ignores_size, 4) }.unwrap());
}

#[test]
fn undersized_serialize_probe_reports_a_rejecting_core() {
    unsafe extern "C" fn requires_exact_size(_buffer: *mut std::ffi::c_void, size: usize) -> bool {
        size == 4
    }

    assert!(unsafe { serialize_rejects_undersized_buffer(requires_exact_size, 4) }.unwrap());
}

#[test]
fn save_ram_snapshot_copies_and_hashes_nonempty_memory() {
    let mut source = [1_u8, 2, 3];
    let snapshot = unsafe { copy_save_ram(source.as_mut_ptr().cast(), source.len()) }.unwrap();
    source.fill(0);

    assert!(snapshot.nonnull);
    assert_eq!(snapshot.bytes, [1, 2, 3]);
    assert_eq!(
        snapshot.hash,
        Some(sha2::Sha256::digest([1_u8, 2, 3]).into())
    );
}

#[test]
fn save_ram_snapshot_allows_null_empty_memory_with_an_empty_hash_sentinel() {
    let snapshot = unsafe { copy_save_ram(std::ptr::null_mut(), 0) }.unwrap();

    assert!(!snapshot.nonnull);
    assert!(snapshot.bytes.is_empty());
    assert!(snapshot.hash.is_none());
}

#[test]
fn save_ram_snapshot_rejects_null_nonempty_memory() {
    let error = unsafe { copy_save_ram(std::ptr::null_mut(), 1) }.unwrap_err();

    assert_eq!(
        error.to_string(),
        "retro_get_memory_data returned null for 1 bytes of save RAM"
    );
}

#[test]
fn environment_supplies_english_and_a_log_callback() {
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState::default());
    let mut language = u32::MAX;
    let mut log = RetroLogCallback { log: None };

    unsafe {
        assert!(environment(
            RETRO_ENVIRONMENT_GET_LANGUAGE,
            std::ptr::from_mut(&mut language).cast(),
        ));
        assert!(environment(
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            std::ptr::from_mut(&mut log).cast(),
        ));
    }
    CALLBACK_STATE.lock().unwrap().take();

    assert_eq!(language, 0);
    assert!(log.log.is_some());
}

#[test]
fn video_hash_ignores_pitch_padding() {
    let tight = [1_u8, 2, 3, 4, 5, 6, 7, 8];
    let padded = [1_u8, 2, 3, 4, 90, 91, 5, 6, 7, 8, 92, 93];
    let changed_pixel = [1_u8, 2, 3, 9, 5, 6, 7, 8];

    let sample = |bytes: &[u8], width, height, pitch| {
        *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
            requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
            active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
            ..CallbackState::default()
        });
        unsafe { video_refresh(bytes.as_ptr().cast(), width, height, pitch) };
        CALLBACK_STATE.lock().unwrap().take().unwrap()
    };

    let tight_sample = sample(&tight, 2, 2, 4);
    let padded_sample = sample(&padded, 2, 2, 6);
    let changed_pixel_sample = sample(&changed_pixel, 2, 2, 4);
    let reshaped_sample = sample(&tight, 1, 4, 2);

    assert_eq!(
        tight_sample.video_hasher.clone().finalize(),
        padded_sample.video_hasher.clone().finalize()
    );
    assert_ne!(
        tight_sample.video_hasher.clone().finalize(),
        changed_pixel_sample.video_hasher.clone().finalize()
    );
    assert_ne!(
        tight_sample.video_hasher.clone().finalize(),
        reshaped_sample.video_hasher.clone().finalize()
    );
    assert_eq!(tight_sample.counts.visible_video_bytes, 8);
    assert_eq!(padded_sample.counts.visible_video_bytes, 8);
    assert_eq!(padded_sample.counts.video_bytes, 12);
}

#[test]
fn video_refresh_rejects_pitch_shorter_than_the_visible_row() {
    let bytes = [0_u8; 8];
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        ..CallbackState::default()
    });

    unsafe { video_refresh(bytes.as_ptr().cast(), 2, 2, 3) };

    assert!(
        CALLBACK_STATE
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .invalid_video_pitch
    );
}

#[test]
fn callback_validation_rejects_zero_pitch_for_a_non_null_visible_frame() {
    let byte = 0_u8;
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState::default());

    unsafe { video_refresh(std::ptr::null(), 1, 1, 0) };
    let duplicate_frame = CALLBACK_STATE.lock().unwrap().take().unwrap();

    assert!(validate_callback_buffers(&duplicate_frame).is_ok());

    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        ..CallbackState::default()
    });

    unsafe { video_refresh(std::ptr::from_ref(&byte).cast(), 1, 1, 0) };
    let callback_state = CALLBACK_STATE.lock().unwrap().take().unwrap();
    let error = validate_callback_buffers(&callback_state).unwrap_err();

    assert_eq!(
        error.to_string(),
        "libretro core supplied pitch 0 for a 1x1 frame in pixel format 2"
    );
}

#[test]
fn video_refresh_rejects_an_unrepresentable_buffer_length() {
    let byte = 0_u8;
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState {
        requested_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        active_pixel_format: RETRO_PIXEL_FORMAT_RGB565,
        ..CallbackState::default()
    });

    unsafe { video_refresh(std::ptr::from_ref(&byte).cast(), 1, 2, usize::MAX) };

    assert!(
        CALLBACK_STATE
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .invalid_video_buffer_len
    );
}

#[test]
fn audio_batch_rejects_an_unrepresentable_buffer_length() {
    let sample = 0_i16;
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState::default());

    assert_eq!(unsafe { audio_batch(&sample, usize::MAX) }, 0);

    assert!(
        CALLBACK_STATE
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .invalid_audio_buffer_len
    );
}

#[test]
fn callback_validation_rejects_an_invalid_audio_buffer_length() {
    let callback_state = CallbackState {
        invalid_audio_buffer_len: true,
        ..CallbackState::default()
    };

    let error = validate_callback_buffers(&callback_state).unwrap_err();

    assert_eq!(
        error.to_string(),
        "libretro core supplied an invalid audio buffer length"
    );
}

#[test]
fn v2_core_options_register_defaults_without_replacing_explicit_values() {
    let speed_key = CString::new("core_speed").unwrap();
    let speed_default = CString::new("balanced").unwrap();
    let speed_override = CString::new("accurate").unwrap();
    let filter_key = CString::new("core_filter").unwrap();
    let filter_default = CString::new("nearest").unwrap();
    let empty_value = RetroCoreOptionValue {
        value: std::ptr::null(),
        label: std::ptr::null(),
    };
    let empty_definition = RetroCoreOptionV2Definition {
        key: std::ptr::null(),
        desc: std::ptr::null(),
        desc_categorized: std::ptr::null(),
        info: std::ptr::null(),
        info_categorized: std::ptr::null(),
        category_key: std::ptr::null(),
        values: [empty_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
        default_value: std::ptr::null(),
    };
    let definitions = [
        RetroCoreOptionV2Definition {
            key: speed_key.as_ptr(),
            desc: std::ptr::null(),
            desc_categorized: std::ptr::null(),
            info: std::ptr::null(),
            info_categorized: std::ptr::null(),
            category_key: std::ptr::null(),
            values: [empty_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
            default_value: speed_default.as_ptr(),
        },
        RetroCoreOptionV2Definition {
            key: filter_key.as_ptr(),
            desc: std::ptr::null(),
            desc_categorized: std::ptr::null(),
            info: std::ptr::null(),
            info_categorized: std::ptr::null(),
            category_key: std::ptr::null(),
            values: [empty_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
            default_value: filter_default.as_ptr(),
        },
        empty_definition,
    ];
    let options = RetroCoreOptionsV2 {
        categories: std::ptr::null(),
        definitions: definitions.as_ptr(),
    };
    let mut callback_state = CallbackState::default();
    callback_state.options.push(CoreOptionValue {
        key: speed_key.to_owned(),
        value: speed_override.to_owned(),
    });
    *CALLBACK_STATE.lock().unwrap() = Some(callback_state);

    unsafe {
        let mut version = 0;
        assert!(environment(
            RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION,
            std::ptr::from_mut(&mut version).cast(),
        ));
        assert_eq!(version, 2);
        assert!(environment(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2,
            std::ptr::from_ref(&options).cast_mut().cast(),
        ));
    }
    let state = CALLBACK_STATE.lock().unwrap().take().unwrap();

    assert_eq!(state.options.len(), 2);
    assert_eq!(state.options[0].value.as_c_str(), speed_override.as_c_str());
    assert_eq!(state.options[1].key.as_c_str(), filter_key.as_c_str());
    assert_eq!(state.options[1].value.as_c_str(), filter_default.as_c_str());
}

#[test]
fn v2_international_core_options_register_english_defaults() {
    let key = CString::new("core_region").unwrap();
    let default = CString::new("ntsc").unwrap();
    let empty_value = RetroCoreOptionValue {
        value: std::ptr::null(),
        label: std::ptr::null(),
    };
    let definitions = [
        RetroCoreOptionV2Definition {
            key: key.as_ptr(),
            desc: std::ptr::null(),
            desc_categorized: std::ptr::null(),
            info: std::ptr::null(),
            info_categorized: std::ptr::null(),
            category_key: std::ptr::null(),
            values: [empty_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
            default_value: default.as_ptr(),
        },
        RetroCoreOptionV2Definition {
            key: std::ptr::null(),
            desc: std::ptr::null(),
            desc_categorized: std::ptr::null(),
            info: std::ptr::null(),
            info_categorized: std::ptr::null(),
            category_key: std::ptr::null(),
            values: [empty_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
            default_value: std::ptr::null(),
        },
    ];
    let options = RetroCoreOptionsV2 {
        categories: std::ptr::null(),
        definitions: definitions.as_ptr(),
    };
    let international = RetroCoreOptionsV2Intl {
        us: std::ptr::from_ref(&options),
        local: std::ptr::null(),
    };
    *CALLBACK_STATE.lock().unwrap() = Some(CallbackState::default());

    unsafe {
        assert!(environment(
            RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL,
            std::ptr::from_ref(&international).cast_mut().cast(),
        ));
    }
    let state = CALLBACK_STATE.lock().unwrap().take().unwrap();

    assert_eq!(state.options.len(), 1);
    assert_eq!(state.options[0].key.as_c_str(), key.as_c_str());
    assert_eq!(state.options[0].value.as_c_str(), default.as_c_str());
}
