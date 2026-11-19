use super::{
    SpriteRaster,
    line_port::{LinePort, ReadWord},
};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

const TAG: &[u8; 8] = b"ZSPRBUF1";
const PAYLOAD_BYTES: usize = 61;

impl SpriteRaster {
    pub fn validate_line_frontier(&self) -> Result<()> {
        self.port.validate(self.output)
    }

    pub fn write_line_tail(&self, writer: &mut StateWriter) -> Result<()> {
        self.validate_line_frontier()?;
        if self.port == LinePort::legacy_unknown() {
            return Ok(());
        }
        writer.write_bytes(TAG);
        writer.write_section(|w| {
            self.port.common.write(w);
            for word in self.port.parity {
                word.write(w);
            }
            w.write_bytes(&self.port.output_known);
            for row in self.port.rows {
                w.write_u8(row.unwrap_or(0));
                w.write_bool(row.is_some());
            }
            w.write_bool(false);
        });
        Ok(())
    }

    pub fn read_line_tail(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        if reader.is_exhausted() {
            return Ok(());
        }
        let mut tag = [0; 8];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == TAG, "unknown sprite line state extension");
        let mut r = StateReader::new(reader.read_slice(PAYLOAD_BYTES)?);
        self.port.common = ReadWord::read(&mut r)?;
        self.port.parity = [ReadWord::read(&mut r)?, ReadWord::read(&mut r)?];
        r.read_exact(&mut self.port.output_known)?;
        for row in &mut self.port.rows {
            let value = r.read_u8()?;
            let known = r.read_bool()?;
            ensure!(
                value < 64 && (known || value == 0),
                "noncanonical sprite data row"
            );
            *row = known.then_some(value);
        }
        ensure!(!r.read_bool()?, "incomplete saved sprite line MCLK edge");
        ensure!(
            r.is_exhausted() && reader.is_exhausted(),
            "trailing sprite line state data"
        );
        self.validate_line_frontier()
    }
}
