use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

pub(super) fn validate_paths(input: &Path, options: &super::HeadlessOptions) -> Result<()> {
    let distinct = crate::cli::audio_discovery::ensure_distinct_output_path;
    if let (Some(dump), Some(trace)) = (&options.audio_dump_path, &options.audio_trace_path) {
        distinct(dump, trace)?;
    }
    for audio in [
        options.audio_dump_path.as_ref(),
        options.audio_trace_path.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        distinct(audio, input)?;
        for other in [
            options.debug_state_path.as_ref(),
            options.screenshot_path.as_ref(),
            options.pce_save_state_path.as_ref(),
            options.coleco_save_state_path.as_ref(),
            options.load_state_path.as_ref(),
            options.replay_path.as_ref(),
            options.replay_peer_path.as_ref(),
            options.tas_project_path.as_ref(),
            options.tas_export_path.as_ref(),
            options.ws_link_peer_path.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            distinct(audio, other)?;
        }
        for directory in [
            options.screenshot_dir.as_ref(),
            options.gba_dump_memory_dir.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            crate::cli::audio_discovery::ensure_outside_output_directory(audio, directory)?;
        }
    }
    Ok(())
}

pub(super) struct StreamingAudioDump {
    path: PathBuf,
    temporary: tempfile::NamedTempFile,
    writer: BufWriter<std::fs::File>,
    sample_rate: u32,
    samples: u64,
}

impl StreamingAudioDump {
    pub(super) fn new(path: &Path, sample_rate: u32) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        let writer = BufWriter::new(temporary.reopen()?);
        Ok(Self {
            path: path.to_path_buf(),
            temporary,
            writer,
            sample_rate,
            samples: 0,
        })
    }

    pub(super) fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        for sample in samples {
            self.writer.write_all(&sample.to_le_bytes())?;
        }
        self.samples = self
            .samples
            .checked_add(samples.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("audio dump sample count overflow"))?;
        Ok(())
    }

    pub(super) fn finish(self) -> Result<()> {
        let Self {
            path,
            temporary,
            mut writer,
            sample_rate,
            samples,
        } = self;
        writer.flush()?;
        drop(writer);
        temporary.persist(&path).map_err(|error| error.error)?;
        println!(
            "[headless] audio-dump={} format=f32le channels=2 sample_rate={} samples={}",
            path.display(),
            sample_rate,
            samples
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_cannot_replace_input_or_another_output_through_a_path_alias() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("input.nes");
        fs::write(&input, b"original")?;
        let mut options = super::super::HeadlessOptions {
            audio_dump_path: Some(directory.path().join(".").join("input.nes")),
            ..Default::default()
        };
        assert!(validate_paths(&input, &options).is_err());
        options.audio_dump_path = Some(directory.path().join("capture.zip"));
        options.audio_trace_path = Some(directory.path().join(".").join("capture.zip"));
        assert!(validate_paths(&input, &options).is_err());
        options.audio_trace_path = None;
        assert!(validate_paths(&input, &options).is_ok());
        options.audio_dump_path = None;
        options.audio_trace_path = Some(directory.path().join("capture.zip"));
        options.screenshot_path = Some(directory.path().join(".").join("capture.zip"));
        assert!(validate_paths(&input, &options).is_err());
        options.audio_trace_path = None;
        options.screenshot_path = None;
        options.screenshot_dir = Some(directory.path().join("shots"));
        options.audio_dump_path = Some(directory.path().join("shots/frame_000001.png"));
        assert!(validate_paths(&input, &options).is_err());
        options.audio_dump_path = Some(directory.path().join("shots-other/audio.f32"));
        assert!(validate_paths(&input, &options).is_ok());
        options.gba_dump_memory_dir = Some(directory.path().join("memory"));
        options.audio_dump_path = Some(directory.path().join("memory/vram.bin"));
        assert!(validate_paths(&input, &options).is_err());
        assert_eq!(fs::read(input)?, b"original");
        Ok(())
    }

    #[test]
    fn successful_dump_replaces_an_existing_destination() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("audio.f32le");
        fs::write(&path, b"previous dump")?;
        let mut dump = StreamingAudioDump::new(&path, 48_000)?;
        dump.write_samples(&[0.5, -0.25])?;
        dump.finish()?;
        let expected = [0.5f32.to_le_bytes(), (-0.25f32).to_le_bytes()].concat();
        assert_eq!(fs::read(path)?, expected);
        Ok(())
    }
}
