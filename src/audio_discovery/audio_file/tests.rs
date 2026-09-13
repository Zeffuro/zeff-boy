use super::*;
use crate::audio_discovery::formats::FormatAvailability;

fn signal(frames: usize, channels: u16) -> Vec<i16> {
    (0..frames * channels as usize)
        .map(|i| ((i as f64 * 0.13).sin() * 12000.0) as i16)
        .collect()
}

#[test]
fn lossless_files_decode_exactly_in_independent_consumers() -> Result<()> {
    for frames in [1, 15, 4096, 4109, 12000] {
        for channels in [1, 2] {
            let pcm = signal(frames, channels);
            for format in [AudioFormat::Wav, AudioFormat::Flac] {
                let data = encode(
                    format,
                    AudioData {
                        pcm: &pcm,
                        channels,
                        sample_rate: 48000,
                        loop_range: (frames > 1).then_some((1, frames as u32)),
                        pitch: Some((60, -13)),
                    },
                    b"{\"title\":\"test\"}",
                    &AtomicBool::new(false),
                )?;
                let decoded = if format == AudioFormat::Wav {
                    let mut reader = hound::WavReader::new(Cursor::new(&data))?;
                    assert_eq!(reader.spec().channels, channels);
                    reader
                        .samples::<i16>()
                        .collect::<std::result::Result<Vec<_>, _>>()?
                } else {
                    let mut reader = claxon::FlacReader::new(Cursor::new(&data))?;
                    assert_eq!(reader.streaminfo().samples, Some(frames as u64));
                    assert_eq!(reader.streaminfo().sample_rate, 48000);
                    assert_eq!(reader.get_tag("PITCH_CORRECTION_CENTS").next(), Some("-13"));
                    reader
                        .samples()
                        .map(|sample| sample.map(|sample| sample as i16))
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                assert_eq!(
                    decoded, pcm,
                    "{format:?} {frames} frames {channels} channels"
                );
            }
        }
    }
    Ok(())
}

#[cfg(feature = "audio-recording")]
#[test]
fn tiny_vorbis_samples_retain_their_exact_frame_counts() -> Result<()> {
    for frames in [1, 15, 64, 4096, 4109] {
        for channels in [1, 2] {
            let pcm = signal(frames, channels);
            let bytes = encode(
                AudioFormat::Ogg,
                AudioData {
                    pcm: &pcm,
                    channels,
                    sample_rate: 8000,
                    loop_range: None,
                    pitch: None,
                },
                b"{}",
                &AtomicBool::new(false),
            )?;
            let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))?;
            let mut count = 0;
            while let Some(block) = reader.read_dec_packet_itl()? {
                count += block.len();
            }
            assert_eq!(count, pcm.len(), "{frames} frames, {channels} channels");
        }
    }
    Ok(())
}

#[cfg(feature = "audio-recording")]
#[test]
fn vorbis_is_decodable_deterministic_and_preserves_large_unicode_comments() -> Result<()> {
    let pcm = signal(16000, 2);
    let metadata = format!("{{\"title\":\"{}\"}}", "音".repeat(24000));
    let run = || {
        encode(
            AudioFormat::Ogg,
            AudioData {
                pcm: &pcm,
                channels: 2,
                sample_rate: 48000,
                loop_range: Some((7, 12000)),
                pitch: Some((60, 3)),
            },
            metadata.as_bytes(),
            &AtomicBool::new(false),
        )
    };
    let bytes = run()?;
    assert_eq!(bytes, run()?);
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(&bytes))?;
    assert_eq!(reader.ident_hdr.audio_channels, 2);
    assert!(
        reader
            .comment_hdr
            .comment_list
            .contains(&("ZEFF_METADATA".to_owned(), metadata))
    );
    assert!(
        reader
            .comment_hdr
            .comment_list
            .contains(&("LOOPEND".to_owned(), "12000".to_owned()))
    );
    let mut decoded = Vec::new();
    while let Some(block) = reader.read_dec_packet_itl()? {
        decoded.extend(block);
    }
    assert_eq!(decoded.len(), pcm.len());
    let error = decoded
        .iter()
        .zip(&pcm)
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum::<f64>()
        / pcm.len() as f64;
    assert!(
        error.sqrt() < 1500.0,
        "unexpected Vorbis distortion: {}",
        error.sqrt()
    );
    Ok(())
}

#[test]
fn long_wav_uses_bounded_streaming_and_exceeds_the_old_memory_limit() -> Result<()> {
    let frames = 720 * 48000;
    let mut source = tempfile::tempfile()?;
    source.set_len(frames * 4)?;
    let mut output = tempfile::tempfile()?;
    encode_to(
        AudioFormat::Wav,
        &mut source,
        AudioInfo {
            frames,
            channels: 2,
            sample_rate: 48000,
            loop_range: None,
            pitch: None,
        },
        b"{}",
        &AtomicBool::new(false),
        &mut output,
    )?;
    assert!(output.metadata()?.len() > 128 * 1024 * 1024);
    output.rewind()?;
    let reader = hound::WavReader::new(output)?;
    assert_eq!(u64::from(reader.duration()), frames);
    Ok(())
}

#[test]
fn malformed_pcm_and_cancellation_fail_before_output() -> Result<()> {
    for format in AudioFormat::ALL
        .into_iter()
        .filter(|format| format.available())
    {
        let pcm = [1, 2, 3];
        assert!(
            encode(
                format,
                AudioData {
                    pcm: &pcm,
                    channels: 2,
                    sample_rate: 48000,
                    loop_range: None,
                    pitch: None
                },
                b"{}",
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let mut output = Cursor::new(Vec::new());
        assert!(
            encode_to(
                format,
                &mut Cursor::new(vec![0; 4]),
                AudioInfo {
                    frames: 1,
                    channels: 2,
                    sample_rate: 48000,
                    loop_range: None,
                    pitch: None
                },
                b"{}",
                &AtomicBool::new(true),
                &mut output
            )
            .is_err()
        );
        assert!(output.into_inner().is_empty());
    }
    Ok(())
}
