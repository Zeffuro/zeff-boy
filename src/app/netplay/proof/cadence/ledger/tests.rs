use super::*;

fn checkpoint(frame: u64) -> Message {
    Message::Checkpoint {
        frame,
        logical: [1; 32],
        video: [2; 32],
        audio: [3; 32],
        persistent: [4; 32],
    }
}

#[test]
fn exact_sequence_preserves_ports_and_raw_pcm_bits() {
    let mut ledger = Ledger::new(3).unwrap();
    let audio = [0.0, -0.0, f32::from_bits(0x7fc0_0123)];
    for frame in 0..3 {
        ledger.record_input(frame, frame as u16 + 7).unwrap();
        ledger.record_input(frame, frame as u16 + 7).unwrap();
        ledger
            .record_frame(&checkpoint(frame + 1), [7, 9], &audio)
            .unwrap();
    }
    ledger.check_complete().unwrap();
    assert_eq!(ledger.inputs(), [7, 8, 9]);
    assert_eq!(ledger.records().len(), 3);
    for (index, record) in ledger.records().iter().enumerate() {
        record
            .check(&checkpoint(index as u64 + 1), [7, 9], &audio)
            .unwrap();
        assert_eq!(record.pcm_bits, [0, 0x8000_0000, 0x7fc0_0123]);
    }
}

#[test]
fn target_and_pcm_limits_are_enforced_before_capture() {
    for target in [0, MAX_FRAMES + 1, u64::MAX] {
        assert!(Ledger::new(target).is_err());
    }
    assert!(Ledger::new(MAX_FRAMES).is_ok());
    let mut ledger = Ledger::new(2).unwrap();
    ledger
        .record_frame(&checkpoint(1), [0; 2], &[0.0; MAX_PCM_SAMPLES])
        .unwrap();
    assert!(
        ledger
            .record_frame(&checkpoint(2), [0; 2], &[0.0; MAX_PCM_SAMPLES + 1])
            .is_err()
    );
    assert_eq!(ledger.records().len(), 1);
    assert!(ledger.record_frame(&checkpoint(2), [0; 2], &[]).is_err());
    assert_eq!(ledger.records().len(), 1);
}

#[test]
fn missing_inputs_or_confirmations_cannot_complete() {
    let mut ledger = Ledger::new(1).unwrap();
    assert!(ledger.check_complete().is_err());
    ledger.record_input(0, 7).unwrap();
    assert!(ledger.check_complete().is_err());
    let mut ledger = Ledger::new(1).unwrap();
    ledger.record_frame(&checkpoint(1), [7, 9], &[]).unwrap();
    assert!(ledger.check_complete().is_err());
}

#[test]
fn local_sample_gaps_stale_retries_and_overflow_are_rejected() {
    for frame in [1, 3, u64::MAX] {
        let mut ledger = Ledger::new(3).unwrap();
        assert!(ledger.record_input(frame, 7).is_err());
        assert!(ledger.inputs().is_empty());
    }
    let mut ledger = Ledger::new(3).unwrap();
    ledger.record_input(0, 7).unwrap();
    ledger.record_input(1, 9).unwrap();
    assert!(ledger.record_input(0, 7).is_err());
    assert_eq!(ledger.inputs(), [7, 9]);
    let mut ledger = Ledger::new(1).unwrap();
    ledger.record_input(0, 7).unwrap();
    ledger.record_input(0, 7).unwrap();
    assert!(ledger.record_input(1, 7).is_err());
    assert_eq!(ledger.inputs(), [7]);
}

#[test]
fn first_error_is_sticky_and_stops_new_recording() {
    let mut ledger = Ledger::new(2).unwrap();
    ledger.record_input(0, 7).unwrap();
    let first = ledger.record_input(0, 9).unwrap_err().to_string();
    assert_eq!(ledger.record_input(1, 11).unwrap_err().to_string(), first);
    assert_eq!(
        ledger
            .record_frame(&checkpoint(1), [0; 2], &[])
            .unwrap_err()
            .to_string(),
        first
    );
    assert_eq!(ledger.record_audio().unwrap_err().to_string(), first);
    assert_eq!(ledger.check_error().unwrap_err().to_string(), first);
    assert_eq!(ledger.check_complete().unwrap_err().to_string(), first);
    assert_eq!(ledger.inputs(), [7]);
    assert!(ledger.records().is_empty());
}

#[test]
fn delayed_frames_keep_sample_and_completed_frame_indices_distinct() {
    let mut ledger = Ledger::new(5).unwrap();
    let samples = [7, 9, 11, 13, 15];
    for (frame, &raw) in samples.iter().enumerate() {
        ledger.record_input(frame as u64, raw).unwrap();
        let ports = if frame < 2 {
            [0, 0]
        } else {
            [samples[frame - 2], 31 + frame as u16]
        };
        ledger
            .record_frame(&checkpoint(frame as u64 + 1), ports, &[])
            .unwrap();
    }
    ledger.check_complete().unwrap();
    assert_eq!(ledger.inputs(), samples);
    assert_eq!(ledger.records()[0].ports, [0, 0]);
    assert_eq!(ledger.records()[1].ports, [0, 0]);
    assert_eq!(ledger.records()[2].ports, [7, 33]);
    assert_eq!(ledger.records()[4].ports, [11, 35]);
}

#[test]
fn full_bounded_capture_accepts_last_retry_and_rejects_extra_frame() {
    let mut ledger = Ledger::new(MAX_FRAMES).unwrap();
    for frame in 0..MAX_FRAMES {
        ledger.record_input(frame, frame as u16).unwrap();
        ledger
            .record_frame(&checkpoint(frame + 1), [frame as u16, 0], &[])
            .unwrap();
    }
    ledger.record_input(MAX_FRAMES - 1, 999).unwrap();
    ledger.check_error().unwrap();
    ledger.check_complete().unwrap();
    assert!(ledger.record_input(MAX_FRAMES, 1000).is_err());
    assert_eq!(ledger.inputs().len(), MAX_FRAMES as usize);
    assert_eq!(ledger.records().len(), MAX_FRAMES as usize);
}

#[test]
fn malformed_duplicate_gapped_and_out_of_range_checkpoints_are_rejected() {
    for message in [
        Message::Close { frame: 1 },
        checkpoint(0),
        checkpoint(2),
        checkpoint(4),
        checkpoint(u64::MAX),
    ] {
        let mut ledger = Ledger::new(3).unwrap();
        assert!(ledger.record_frame(&message, [0; 2], &[]).is_err());
        assert!(ledger.records().is_empty());
    }
    for frame in [1, 3] {
        let mut ledger = Ledger::new(3).unwrap();
        ledger.record_frame(&checkpoint(1), [0; 2], &[]).unwrap();
        assert!(
            ledger
                .record_frame(&checkpoint(frame), [0; 2], &[])
                .is_err()
        );
        assert_eq!(ledger.records().len(), 1);
    }
    let mut ledger = Ledger::new(1).unwrap();
    ledger.record_frame(&checkpoint(1), [0; 2], &[]).unwrap();
    assert!(ledger.record_frame(&checkpoint(2), [0; 2], &[]).is_err());
    assert_eq!(ledger.records().len(), 1);
}

#[test]
fn audio_only_confirmation_fails_without_allocating_records() {
    let mut ledger = Ledger::new(1).unwrap();
    assert!(ledger.record_audio().is_err());
    assert!(ledger.record_frame(&checkpoint(1), [0; 2], &[]).is_err());
    assert!(ledger.records().is_empty());
}

#[test]
fn reference_comparison_rejects_each_checkpoint_field_ports_and_pcm_changes() {
    let mut ledger = Ledger::new(1).unwrap();
    ledger
        .record_frame(&checkpoint(1), [7, 9], &[0.0, -0.0])
        .unwrap();
    let record = &ledger.records()[0];
    for field in 0..5 {
        let mut expected = checkpoint(1);
        let Message::Checkpoint {
            frame,
            logical,
            video,
            audio,
            persistent,
        } = &mut expected
        else {
            unreachable!();
        };
        match field {
            0 => *frame += 1,
            1 => logical[0] ^= 1,
            2 => video[0] ^= 1,
            3 => audio[0] ^= 1,
            _ => persistent[0] ^= 1,
        }
        assert!(record.check(&expected, [7, 9], &[0.0, -0.0]).is_err());
    }
    assert!(record.check(&checkpoint(1), [9, 7], &[0.0, -0.0]).is_err());
    assert!(record.check(&checkpoint(1), [7, 9], &[0.0, 0.0]).is_err());
    assert!(record.check(&checkpoint(1), [7, 9], &[0.0]).is_err());
    assert!(
        record
            .check(&checkpoint(1), [7, 9], &[0.0, -0.0, 0.0])
            .is_err()
    );
}
