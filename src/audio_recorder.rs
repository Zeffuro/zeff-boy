use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub(crate) struct AudioRecorder {
    writer: BufWriter<File>,
    path: PathBuf,
    sample_rate: u32,
    channels: u16,
    samples_written: u64,
}

impl AudioRecorder {
    pub(crate) fn start(path: &Path, sample_rate: u32) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let header = [0u8; 44];
        writer.write_all(&header)?;

        Ok(Self {
            writer,
            path: path.to_path_buf(),
            sample_rate,
            channels: 2,
            samples_written: 0,
        })
    }

    pub(crate) fn write_samples(&mut self, samples: &[f32]) {
        for &sample in samples {
            let s16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            let _ = self.writer.write_all(&s16.to_le_bytes());
            self.samples_written += 1;
        }
    }

    pub(crate) fn finish(mut self) -> std::io::Result<PathBuf> {
        self.writer.flush()?;
        drop(self.writer);

        let data_size = self.samples_written * 2;
        let file_size = 36 + data_size;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.path)?;

        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(0))?;

        let mut header = Vec::with_capacity(44);

        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(file_size as u32).to_le_bytes());
        header.extend_from_slice(b"WAVE");

        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&self.channels.to_le_bytes());
        header.extend_from_slice(&self.sample_rate.to_le_bytes());
        let byte_rate = self.sample_rate * self.channels as u32 * 2;
        header.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = self.channels * 2;
        header.extend_from_slice(&block_align.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes());

        header.extend_from_slice(b"data");
        header.extend_from_slice(&(data_size as u32).to_le_bytes());

        file.write_all(&header)?;
        file.flush()?;

        Ok(self.path)
    }
}

