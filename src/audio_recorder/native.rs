use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::settings::AudioRecordingFormat;

use crate::audio_tooling::{AudioRecordingContext, AudioSemanticFrame};

use super::AudioTimelineDiscontinuity;

const MIDI_INITIAL_SNAPSHOT_CAPACITY: usize = 3600;

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
    #[cfg(feature = "audio-recording")]
    Ogg {
        writer: BufWriter<File>,
        encoder: vorbis_encoder::Encoder,
        buffer: Vec<f32>,
        chunk_threshold: usize,
    },
    Midi {
        frames: Vec<AudioSemanticFrame>,
    },
    ZeffEvents {
        writer: Option<super::events::ZeffAudioEventWriter>,
        error: Option<std::io::Error>,
    },
}

impl AudioRecorder {
    pub(crate) fn start(
        path: &Path,
        sample_rate: u32,
        format: AudioRecordingFormat,
        context: Option<AudioRecordingContext>,
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
                #[cfg(feature = "audio-recording")]
                {
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
                #[cfg(not(feature = "audio-recording"))]
                {
                    return Err(std::io::Error::other(
                        "OGG Vorbis recording requires the `audio-recording` feature",
                    ));
                }
            }
            AudioRecordingFormat::Midi => RecorderInner::Midi {
                frames: Vec::with_capacity(MIDI_INITIAL_SNAPSHOT_CAPACITY),
            },
            AudioRecordingFormat::ZeffEvents => RecorderInner::ZeffEvents {
                writer: Some(super::events::ZeffAudioEventWriter::start(
                    path,
                    context.ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Zeff audio events require an explicit audio topology",
                        )
                    })?,
                )?),
                error: None,
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
            #[cfg(feature = "audio-recording")]
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
            RecorderInner::Midi { .. } | RecorderInner::ZeffEvents { .. } => {}
        }
    }

    pub(crate) fn write_audio_semantic_frame(&mut self, frame: AudioSemanticFrame) {
        match &mut self.inner {
            RecorderInner::Midi { frames } => frames.push(frame),
            RecorderInner::ZeffEvents { writer, error } if error.is_none() => {
                if let Err(write_error) = writer
                    .as_mut()
                    .expect("active Zeff event recorder owns its writer")
                    .write_frame(&frame)
                {
                    *error = Some(write_error);
                }
            }
            RecorderInner::Wav { .. } | RecorderInner::ZeffEvents { .. } => {}
            #[cfg(feature = "audio-recording")]
            RecorderInner::Ogg { .. } => {}
        }
    }

    pub(crate) fn begin_semantic_timeline_epoch(&mut self, reason: AudioTimelineDiscontinuity) {
        if let RecorderInner::ZeffEvents { writer, error } = &mut self.inner
            && error.is_none()
            && let Err(write_error) = writer
                .as_mut()
                .expect("active Zeff event recorder owns its writer")
                .begin_epoch(reason)
        {
            *error = Some(write_error);
        }
    }

    pub(crate) fn captures_semantics(&self) -> bool {
        self.format.captures_semantics()
    }

    pub(crate) fn supports_uncapped_recording(&self) -> bool {
        self.format.supports_uncapped_recording()
    }

    pub(crate) fn finish(self) -> std::io::Result<PathBuf> {
        match self.inner {
            RecorderInner::Wav {
                writer,
                sample_rate,
                channels,
                samples_written,
                is_float,
            } => finish_wav(
                self.path,
                writer,
                sample_rate,
                channels,
                samples_written,
                is_float,
            ),
            #[cfg(feature = "audio-recording")]
            RecorderInner::Ogg {
                writer,
                encoder,
                buffer,
                ..
            } => finish_ogg(self.path, writer, encoder, &buffer),
            RecorderInner::Midi { frames } => super::midi::finish_midi(self.path, &frames),
            RecorderInner::ZeffEvents { writer, error } => {
                if let Some(error) = error {
                    Err(error)
                } else {
                    writer
                        .expect("active Zeff event recorder owns its writer")
                        .finish()
                }
            }
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

    let (fmt_code, bits_per_sample, bytes_per_sample): (u16, u16, u32) =
        if is_float { (3, 32, 4) } else { (1, 16, 2) };

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

#[cfg(feature = "audio-recording")]
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
