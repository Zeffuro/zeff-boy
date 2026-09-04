use super::*;

#[test]
fn resampler_state_size_stays_fixed_across_multi_hour_clock_history() {
    const SIMULATED_SECONDS: u64 = 3 * 60 * 60;
    let frame_count = (SIMULATED_SECONDS * PSG_INTERNAL_CLOCK_NUMERATOR)
        .div_ceil(u64::from(BLIP_FRAME_CLOCKS) * PSG_CLOCK_DENOMINATOR);
    let mut resampler = StereoBlipResampler::new(1);
    let sample_cells = (
        resampler.left.state_sample_count(),
        resampler.right.state_sample_count(),
    );
    let mut initial_writer = StateWriter::new();
    resampler.write_state(&mut initial_writer);
    let initial_len = initial_writer.position();
    let mut output = Vec::new();

    for frame in 0..frame_count {
        resampler.clocks = BLIP_FRAME_CLOCKS - 1;
        let level = if frame & 1 == 0 {
            MIX_SCALE
        } else {
            -MIX_SCALE
        };
        resampler.push_level(level, -level, &mut output);
        output.clear();
    }

    let mut final_writer = StateWriter::new();
    resampler.write_state(&mut final_writer);
    assert_eq!(final_writer.position(), initial_len);
    assert_eq!(
        (
            resampler.left.state_sample_count(),
            resampler.right.state_sample_count()
        ),
        sample_cells
    );
}

#[test]
fn resampler_state_rejects_malformed_fixed_buffer_metadata() {
    let resampler = StereoBlipResampler::new(44_100);
    let mut writer = StateWriter::new();
    resampler.write_state(&mut writer);
    let bytes = writer.into_bytes();

    let mut bad_clock = bytes.clone();
    bad_clock[..4].copy_from_slice(&BLIP_FRAME_CLOCKS.to_le_bytes());
    assert!(StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_clock)).is_err());

    let mut bad_factor = bytes.clone();
    bad_factor[12..20].copy_from_slice(&0_u64.to_le_bytes());
    assert!(
        StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_factor))
            .unwrap_err()
            .to_string()
            .contains("factor")
    );

    let mut oversized = bytes.clone();
    oversized[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        StereoBlipResampler::read_state(44_100, &mut StateReader::new(&oversized))
            .unwrap_err()
            .to_string()
            .contains("buffer length")
    );

    let mut bad_available = bytes.clone();
    bad_available[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_available))
            .unwrap_err()
            .to_string()
            .contains("available-sample")
    );

    let left_cells = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let right_offset = 12 + 28 + left_cells * 4 + 8;
    let mut mismatched_stereo = bytes.clone();
    mismatched_stereo[right_offset..right_offset + 8].copy_from_slice(&0_u64.to_le_bytes());
    assert!(
        StereoBlipResampler::read_state(44_100, &mut StateReader::new(&mismatched_stereo))
            .unwrap_err()
            .to_string()
            .contains("differs between channels")
    );

    let mut truncated = bytes;
    truncated.pop();
    assert!(StereoBlipResampler::read_state(44_100, &mut StateReader::new(&truncated)).is_err());
}

#[test]
fn psg_state_rejects_oversized_queued_pcm() {
    let mut psg = HuC6280Psg::new();
    psg.set_sample_rate(192_000);
    psg.audio_samples.resize(MAX_PSG_STATE_AUDIO_SAMPLES, 0);
    let mut writer = StateWriter::new();
    psg.write_state(&mut writer);
    assert!(writer.position() <= MAX_PSG_STATE_SECTION_BYTES);

    psg.audio_samples.resize(MAX_PSG_STATE_AUDIO_SAMPLES + 2, 0);
    assert!(
        psg.validate_v1_state()
            .unwrap_err()
            .to_string()
            .contains("queued audio")
    );
}
