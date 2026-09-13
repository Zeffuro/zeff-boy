use std::io::{Cursor, Write};

use anyhow::{Result, ensure};

const LIMIT: usize = 128 * 1024 * 1024;

pub(super) struct Bundle {
    writer: zip::ZipWriter<Cursor<Vec<u8>>>,
    payload_bytes: usize,
    names: std::collections::BTreeSet<String>,
}

impl Bundle {
    pub(super) fn new() -> Self {
        Self {
            writer: zip::ZipWriter::new(Cursor::new(Vec::new())),
            payload_bytes: 0,
            names: Default::default(),
        }
    }

    pub(super) fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        ensure!(
            !name.is_empty()
                && !name.starts_with('/')
                && !name.contains(['\\', ':'])
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "invalid export bundle entry path"
        );
        ensure!(
            self.names.insert(name.to_owned()),
            "duplicate export bundle path"
        );
        self.payload_bytes = self
            .payload_bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= LIMIT - 1024 * 1024)
            .ok_or_else(|| anyhow::anyhow!("export bundle exceeds its size limit"))?;
        self.writer.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )?;
        self.writer.write_all(bytes)?;
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Vec<u8>> {
        let bytes = self.writer.finish()?.into_inner();
        ensure!(bytes.len() <= LIMIT, "export bundle exceeds its size limit");
        Ok(bytes)
    }
}
