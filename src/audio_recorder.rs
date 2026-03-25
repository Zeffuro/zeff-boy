use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::hardware::apu::ApuChannelSnapshot;
use crate::settings::AudioRecordingFormat;

pub(crate) struct AudioRecorder {
    inner: RecorderInner,
    path: PathBuf,
    format: AudioRecordingFormat,
}

enum RecorderInner {
    Wav {
        writer: BufWriter<File>,
        sample_rate: u32,
        channels: u16,
        samples_written: u64,
        is_float: bool,
    },
    Ogg {
        writer: BufWriter<File>,
        encoder: vorbis_encoder::Encoder,
        buffer: Vec<f32>,
        chunk_threshold: usize,
    },
    Midi {
        snapshots: Vec<ApuChannelSnapshot>,
    },
}

impl AudioRecorder {
    pub(crate) fn start(
        path: &Path,
        sample_rate: u32,
        format: AudioRecordingFormat,
    ) -> std::io::Result<Self> {
        let inner = match format {
            AudioRecordingFormat::Wav16 | AudioRecordingFormat::WavFloat => {
                let file = File::create(path)?;
                let mut writer = BufWriter::new(file);
                let header = [0u8; 44];
                writer.write_all(&header)?;
                RecorderInner::Wav {
                    writer,
                    sample_rate,
                    channels: 2,
                    samples_written: 0,
                    is_float: matches!(format, AudioRecordingFormat::WavFloat),
                }
            }
            AudioRecordingFormat::OggVorbis => {
                let file = File::create(path)?;
                let writer = BufWriter::new(file);
                let encoder = vorbis_encoder::Encoder::new(2, sample_rate as u64, 0.6)
                    .map_err(|e| std::io::Error::other(format!("Vorbis init error: {e}")))?;
                let chunk_threshold = sample_rate as usize * 2;
                RecorderInner::Ogg {
                    writer,
                    encoder,
                    buffer: Vec::with_capacity(chunk_threshold),
                    chunk_threshold,
                }
            }
            AudioRecordingFormat::Midi => RecorderInner::Midi {
                snapshots: Vec::with_capacity(3600),
            },
        };

        Ok(Self {
            inner,
            path: path.to_path_buf(),
            format,
        })
    }

    pub(crate) fn write_samples(&mut self, samples: &[f32]) {
        match &mut self.inner {
            RecorderInner::Wav {
                writer,
                samples_written,
                is_float,
                ..
            } => {
                if *is_float {
                    for &sample in samples {
                        let _ = writer.write_all(&sample.to_le_bytes());
                        *samples_written += 1;
                    }
                } else {
                    for &sample in samples {
                        let s16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        let _ = writer.write_all(&s16.to_le_bytes());
                        *samples_written += 1;
                    }
                }
            }
            RecorderInner::Ogg {
                writer,
                encoder,
                buffer,
                chunk_threshold,
            } => {
                buffer.extend_from_slice(samples);
                while buffer.len() >= *chunk_threshold {
                    let chunk: Vec<i16> = buffer
                        .drain(..*chunk_threshold)
                        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();
                    if let Ok(encoded) = encoder.encode(&chunk) {
                        let _ = writer.write_all(&encoded);
                    }
                }
            }
            RecorderInner::Midi { .. } => {
            }
        }
    }

    pub(crate) fn write_apu_snapshot(&mut self, snapshot: ApuChannelSnapshot) {
        if let RecorderInner::Midi { snapshots } = &mut self.inner {
            snapshots.push(snapshot);
        }
    }

    pub(crate) fn is_midi(&self) -> bool {
        self.format.is_midi()
    }

    pub(crate) fn finish(self) -> std::io::Result<PathBuf> {
        match self.inner {
            RecorderInner::Wav {
                writer,
                sample_rate,
                channels,
                samples_written,
                is_float,
            } => finish_wav(self.path, writer, sample_rate, channels, samples_written, is_float),
            RecorderInner::Ogg {
                writer,
                encoder,
                buffer,
                ..
            } => finish_ogg(self.path, writer, encoder, &buffer),
            RecorderInner::Midi { snapshots } => finish_midi(self.path, &snapshots),
        }
    }
}


fn finish_wav(
    path: PathBuf,
    mut writer: BufWriter<File>,
    sample_rate: u32,
    channels: u16,
    samples_written: u64,
    is_float: bool,
) -> std::io::Result<PathBuf> {
    writer.flush()?;
    drop(writer);

    let (fmt_code, bits_per_sample, bytes_per_sample): (u16, u16, u32) = if is_float {
        (3, 32, 4)
    } else {
        (1, 16, 2)
    };

    let data_size = samples_written * bytes_per_sample as u64;
    let file_size = 36 + data_size;

    let mut file = std::fs::OpenOptions::new().write(true).open(&path)?;

    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(0))?;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(file_size as u32).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&fmt_code.to_le_bytes());
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
    header.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * bytes_per_sample as u16;
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(data_size as u32).to_le_bytes());

    file.write_all(&header)?;
    file.flush()?;

    Ok(path)
}

fn finish_ogg(
    path: PathBuf,
    mut writer: BufWriter<File>,
    mut encoder: vorbis_encoder::Encoder,
    remaining: &[f32],
) -> std::io::Result<PathBuf> {
    if !remaining.is_empty() {
        let samples_i16: Vec<i16> = remaining
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        let encoded = encoder
            .encode(&samples_i16)
            .map_err(|e| std::io::Error::other(format!("Vorbis encode error: {e}")))?;
        writer.write_all(&encoded)?;
    }

    let final_data = encoder
        .flush()
        .map_err(|e| std::io::Error::other(format!("Vorbis flush error: {e}")))?;
    writer.write_all(&final_data)?;
    writer.flush()?;

    Ok(path)
}

fn gb_square_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    131072.0 / denom as f64
}

fn gb_wave_freq_to_hz(freq_reg: u16) -> f64 {
    let denom = 2048u32.saturating_sub(freq_reg as u32).max(1);
    65536.0 / denom as f64
}

fn hz_to_midi_note(hz: f64) -> u8 {
    if hz <= 0.0 {
        return 0;
    }
    let note = 69.0 + 12.0 * (hz / 440.0).log2();
    (note.round() as i32).clamp(0, 127) as u8
}

fn volume_to_velocity(vol: u8) -> u8 {
    ((vol as u16 * 127 + 7) / 15).min(127) as u8
}

fn wave_level_to_velocity(level: u8) -> u8 {
    match level {
        0 => 0,
        1 => 127,
        2 => 80,
        3 => 48,
        _ => 0,
    }
}

fn finish_midi(path: PathBuf, snapshots: &[ApuChannelSnapshot]) -> std::io::Result<PathBuf> {
    if snapshots.is_empty() {
        std::fs::write(&path, [])?;
        return Ok(path);
    }

    const TICKS_PER_FRAME: u16 = 1;
    const TEMPO_US_PER_BEAT: u32 = 16742;

    let track_data: Vec<Vec<u8>> = (0..4)
        .map(|ch| build_midi_track(snapshots, ch, TEMPO_US_PER_BEAT))
        .collect();

    let mut smf = Vec::with_capacity(snapshots.len() * 16);

    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes());
    smf.extend_from_slice(&1u16.to_be_bytes());
    smf.extend_from_slice(&5u16.to_be_bytes());
    smf.extend_from_slice(&TICKS_PER_FRAME.to_be_bytes());

    let tempo_track = build_tempo_track(TEMPO_US_PER_BEAT);
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(tempo_track.len() as u32).to_be_bytes());
    smf.extend_from_slice(&tempo_track);

    for track in &track_data {
        smf.extend_from_slice(b"MTrk");
        smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
        smf.extend_from_slice(track);
    }

    std::fs::write(&path, &smf)?;
    Ok(path)
}

fn build_tempo_track(tempo_us: u32) -> Vec<u8> {
    let mut data = Vec::new();

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x51);
    data.push(0x03);
    data.push((tempo_us >> 16) as u8);
    data.push((tempo_us >> 8) as u8);
    data.push(tempo_us as u8);

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

fn midi_program_for_channel(channel: usize) -> Option<u8> {
    match channel {
        0 | 1 => Some(80),
        2 => Some(81),
        3 => None,
        _ => None,
    }
}

fn drum_note_for_noise(volume: u8) -> u8 {
    match volume {
        0..=4 => 36,
        5..=9 => 38,
        _ => 42,
    }
}

fn build_midi_track(snapshots: &[ApuChannelSnapshot], channel: usize, _tempo: u32) -> Vec<u8> {
    let mut data = Vec::new();

    let midi_ch: u8 = match channel {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 9,
        _ => 0,
    };

    let name = match channel {
        0 => "GB CH1 (Square 1)",
        1 => "GB CH2 (Square 2)",
        2 => "GB CH3 (Wave)",
        3 => "GB CH4 (Noise)",
        _ => "Unknown",
    };

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x03);
    write_vlq(&mut data, name.len() as u32);
    data.extend_from_slice(name.as_bytes());

    if let Some(program) = midi_program_for_channel(channel) {
        write_vlq(&mut data, 0);
        data.push(0xC0 | midi_ch);
        data.push(program);
    }

    let mut current_note: Option<u8> = None;
    let mut current_velocity: u8 = 0;
    let mut pending_delta: u32 = 0;

    for snap in snapshots {
        let (enabled, freq_reg, vol_raw) = match channel {
            0 => (snap.ch1_enabled, snap.ch1_frequency, snap.ch1_volume),
            1 => (snap.ch2_enabled, snap.ch2_frequency, snap.ch2_volume),
            2 => (snap.ch3_enabled, snap.ch3_frequency, 0),
            3 => (snap.ch4_enabled, 0, snap.ch4_volume),
            _ => (false, 0, 0),
        };

        let velocity = if channel == 2 {
            wave_level_to_velocity(snap.ch3_output_level)
        } else {
            volume_to_velocity(vol_raw)
        };

        let note = if channel == 3 {
            drum_note_for_noise(vol_raw)
        } else {
            let hz = if channel == 2 {
                gb_wave_freq_to_hz(freq_reg)
            } else {
                gb_square_freq_to_hz(freq_reg)
            };
            hz_to_midi_note(hz)
        };

        let should_sound = enabled && velocity > 0;

        if should_sound {
            if let Some(prev_note) = current_note {
                if prev_note != note {
                    write_vlq(&mut data, pending_delta);
                    data.push(0x80 | midi_ch);
                    data.push(prev_note);
                    data.push(0);

                    pending_delta = 0;

                    write_vlq(&mut data, 0);
                    data.push(0x90 | midi_ch);
                    data.push(note);
                    data.push(velocity);

                    current_note = Some(note);
                    current_velocity = velocity;
                } else if velocity != current_velocity && channel != 3 {
                    write_vlq(&mut data, pending_delta);
                    data.push(0xA0 | midi_ch);
                    data.push(note);
                    data.push(velocity);

                    pending_delta = 0;
                    current_velocity = velocity;
                }
            } else {
                write_vlq(&mut data, pending_delta);
                data.push(0x90 | midi_ch);
                data.push(note);
                data.push(velocity);

                pending_delta = 0;
                current_note = Some(note);
                current_velocity = velocity;
            }
        } else if let Some(prev_note) = current_note.take() {
            write_vlq(&mut data, pending_delta);
            data.push(0x80 | midi_ch);
            data.push(prev_note);
            data.push(0);

            pending_delta = 0;
            current_velocity = 0;
        }

        pending_delta = pending_delta.saturating_add(1);
    }

    if let Some(prev_note) = current_note {
        write_vlq(&mut data, pending_delta);
        data.push(0x80 | midi_ch);
        data.push(prev_note);
        data.push(0);
    }

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

fn write_vlq(buf: &mut Vec<u8>, mut value: u32) {
    if value == 0 {
        buf.push(0);
        return;
    }

    let mut bytes = [0u8; 4];
    let mut count = 0;

    while value > 0 {
        bytes[count] = (value & 0x7F) as u8;
        value >>= 7;
        count += 1;
    }

    for i in (1..count).rev() {
        buf.push(bytes[i] | 0x80);
    }
    buf.push(bytes[0]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(ch1_enabled: bool, ch1_frequency: u16, ch1_volume: u8) -> ApuChannelSnapshot {
        ApuChannelSnapshot {
            ch1_enabled,
            ch1_frequency,
            ch1_volume,
            ch2_enabled: false,
            ch2_frequency: 0,
            ch2_volume: 0,
            ch3_enabled: false,
            ch3_frequency: 0,
            ch3_output_level: 0,
            ch4_enabled: false,
            ch4_volume: 0,
        }
    }

    fn read_vlq(bytes: &[u8], i: &mut usize) -> u32 {
        let mut value = 0u32;
        loop {
            let b = bytes[*i];
            *i += 1;
            value = (value << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                break;
            }
        }
        value
    }

    fn ch0_note_events(track: &[u8]) -> Vec<(u32, u8, u8)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < track.len() {
            let delta = read_vlq(track, &mut i);
            if i >= track.len() {
                break;
            }
            let status = track[i];
            i += 1;
            match status {
                0x90 | 0x80 => {
                    if i + 1 >= track.len() {
                        break;
                    }
                    let note = track[i];
                    let vel = track[i + 1];
                    i += 2;
                    out.push((delta, status, note));
                    let _ = vel;
                }
                0xA0 => {
                    i += 2;
                }
                0xC0 => {
                    i += 1;
                }
                0xFF => {
                    if i >= track.len() {
                        break;
                    }
                    let _meta = track[i];
                    i += 1;
                    let len = read_vlq(track, &mut i) as usize;
                    i = i.saturating_add(len);
                }
                _ => break,
            }
        }
        out
    }

    #[test]
    fn gb_square_freq_to_hz_middle_range() {
        let hz = gb_square_freq_to_hz(1750);
        assert!((hz - 439.8).abs() < 1.0);
    }

    #[test]
    fn gb_wave_freq_to_hz_middle_range() {
        let hz = gb_wave_freq_to_hz(1750);
        assert!((hz - 219.9).abs() < 1.0);
    }

    #[test]
    fn hz_to_midi_note_a4() {
        assert_eq!(hz_to_midi_note(440.0), 69);
    }

    #[test]
    fn hz_to_midi_note_c4() {
        assert_eq!(hz_to_midi_note(261.63), 60);
    }

    #[test]
    fn hz_to_midi_note_zero_returns_zero() {
        assert_eq!(hz_to_midi_note(0.0), 0);
    }

    #[test]
    fn volume_to_velocity_full() {
        assert_eq!(volume_to_velocity(15), 127);
    }

    #[test]
    fn volume_to_velocity_zero() {
        assert_eq!(volume_to_velocity(0), 0);
    }

    #[test]
    fn wave_level_to_velocity_values() {
        assert_eq!(wave_level_to_velocity(0), 0);
        assert_eq!(wave_level_to_velocity(1), 127);
        assert_eq!(wave_level_to_velocity(2), 80);
        assert_eq!(wave_level_to_velocity(3), 48);
    }

    #[test]
    fn write_vlq_zero() {
        let mut buf = Vec::new();
        write_vlq(&mut buf, 0);
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn write_vlq_small() {
        let mut buf = Vec::new();
        write_vlq(&mut buf, 0x7F);
        assert_eq!(buf, vec![0x7F]);
    }

    #[test]
    fn write_vlq_two_bytes() {
        let mut buf = Vec::new();
        write_vlq(&mut buf, 0x80);
        assert_eq!(buf, vec![0x81, 0x00]);
    }

    #[test]
    fn finish_midi_produces_valid_smf_header() {
        let snapshots = vec![
            ApuChannelSnapshot {
                ch1_enabled: true,
                ch1_frequency: 1750,
                ch1_volume: 15,
                ch2_enabled: false,
                ch2_frequency: 0,
                ch2_volume: 0,
                ch3_enabled: false,
                ch3_frequency: 0,
                ch3_output_level: 0,
                ch4_enabled: false,
                ch4_volume: 0,
            },
            ApuChannelSnapshot {
                ch1_enabled: true,
                ch1_frequency: 1800,
                ch1_volume: 12,
                ch2_enabled: false,
                ch2_frequency: 0,
                ch2_volume: 0,
                ch3_enabled: false,
                ch3_frequency: 0,
                ch3_output_level: 0,
                ch4_enabled: false,
                ch4_volume: 0,
            },
        ];

        let dir = std::env::temp_dir();
        let path = dir.join("test_midi_output.mid");
        let result = finish_midi(path.clone(), &snapshots);
        assert!(result.is_ok());

        let data = std::fs::read(&path).unwrap();
        assert!(data.len() > 14);
        assert_eq!(&data[0..4], b"MThd");
        assert_eq!(&data[14..18], b"MTrk");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn midi_note_change_on_adjacent_snapshots_advances_time() {
        let snapshots = vec![
            snapshot(true, 1750, 15),
            snapshot(true, 1800, 15),
        ];

        let track = build_midi_track(&snapshots, 0, 0);
        let events = ch0_note_events(&track);
        assert!(events.len() >= 4);

        assert_eq!(events[0].0, 0);
        assert_eq!(events[0].1, 0x90);
        assert_eq!(events[1].0, 1);
        assert_eq!(events[1].1, 0x80);
        assert_eq!(events[2].0, 0);
        assert_eq!(events[2].1, 0x90);
    }

    #[test]
    fn midi_note_off_on_adjacent_snapshot_advances_time() {
        let snapshots = vec![
            snapshot(true, 1750, 15),
            snapshot(false, 1750, 15),
        ];

        let track = build_midi_track(&snapshots, 0, 0);
        let events = ch0_note_events(&track);
        assert!(events.len() >= 2);

        assert_eq!(events[0], (0, 0x90, events[0].2));
        assert_eq!(events[1], (1, 0x80, events[1].2));
    }

    #[test]
    fn midi_new_note_after_one_silent_snapshot_uses_delta_one() {
        let snapshots = vec![
            snapshot(false, 1750, 15),
            snapshot(true, 1750, 15),
        ];

        let track = build_midi_track(&snapshots, 0, 0);
        let events = ch0_note_events(&track);
        assert!(!events.is_empty());

        assert_eq!(events[0].0, 1);
        assert_eq!(events[0].1, 0x90);
    }
}
