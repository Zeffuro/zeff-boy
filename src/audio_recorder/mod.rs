mod midi;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use zeff_gb_core::hardware::apu::ApuChannelSnapshot as GbApuChannelSnapshot;
use zeff_nes_core::hardware::apu::ApuChannelSnapshot as NesApuChannelSnapshot;
use crate::settings::AudioRecordingFormat;

#[derive(Clone, Copy, Debug)]
pub(crate) enum MidiApuSnapshot {
    Gb(GbApuChannelSnapshot),
    Nes(NesApuChannelSnapshot),
}

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
        snapshots: Vec<MidiApuSnapshot>,
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

    pub(crate) fn write_apu_snapshot(&mut self, snapshot: MidiApuSnapshot) {
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
            RecorderInner::Midi { snapshots } => midi::finish_midi(self.path, &snapshots),
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

