use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

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
