use super::serial::SerialDevice;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

/// Maximum packed barcode bitstream accepted from the host.
pub const MAX_BARDIGUN_SCAN_BYTES: usize = 4 * 1024;

#[derive(Debug, Default)]
pub struct BardigunBarcodeReader {
    scan: Vec<u8>,
    position: usize,
}

impl BardigunBarcodeReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_scan(&mut self, scan: Vec<u8>) -> Result<()> {
        validate_scan_len(scan.len())?;
        self.scan = scan;
        self.position = 0;
        Ok(())
    }

    pub fn pending_bytes(&self) -> usize {
        self.scan.len().saturating_sub(self.position)
    }

    pub(crate) fn reconnect(&mut self) {
        self.scan.clear();
        self.position = 0;
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.scan.len() as u64);
        writer.write_bytes(&self.scan);
        writer.write_u64(self.position as u64);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let len = read_bounded_len(reader)?;
        let mut scan = vec![0; len];
        reader.read_exact(&mut scan)?;

        let position = reader.read_u64()?;
        let position = usize::try_from(position)
            .map_err(|_| anyhow::anyhow!("Bardigun scan position does not fit usize"))?;
        if scan.is_empty() {
            if position != 0 {
                bail!("idle Bardigun scan has a nonzero position");
            }
        } else if position >= scan.len() {
            bail!("Bardigun scan position {position} is outside its {len}-byte bitstream");
        }

        Ok(Self { scan, position })
    }
}

impl SerialDevice for BardigunBarcodeReader {
    fn exchange_byte(&mut self, _byte: u8) -> u8 {
        let Some(&response) = self.scan.get(self.position) else {
            return 0;
        };

        self.position += 1;
        if self.position == self.scan.len() {
            self.reconnect();
        }
        response
    }
}

fn validate_scan_len(len: usize) -> Result<()> {
    if len == 0 {
        bail!("Bardigun barcode scan cannot be empty");
    }
    if len > MAX_BARDIGUN_SCAN_BYTES {
        bail!("Bardigun barcode scan is {len} bytes; maximum is {MAX_BARDIGUN_SCAN_BYTES}");
    }
    Ok(())
}

fn read_bounded_len(reader: &mut StateReader<'_>) -> Result<usize> {
    let len = reader.read_u64()?;
    let len = usize::try_from(len)
        .map_err(|_| anyhow::anyhow!("Bardigun scan length does not fit usize"))?;
    if len > MAX_BARDIGUN_SCAN_BYTES {
        bail!("Bardigun scan length {len} exceeds maximum {MAX_BARDIGUN_SCAN_BYTES}");
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(reader: &mut BardigunBarcodeReader, bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .map(|&byte| reader.exchange_byte(byte))
            .collect()
    }

    #[test]
    fn protocol_transcript_is_idle_zero_then_queued_bytes_then_zero() {
        let mut reader = BardigunBarcodeReader::new();
        assert_eq!(exchange(&mut reader, &[0xFF, 0xFF]), [0, 0]);

        reader.queue_scan(vec![0x81, 0x24, 0xA5]).unwrap();

        assert_eq!(exchange(&mut reader, &[0xFF; 5]), [0x81, 0x24, 0xA5, 0, 0]);
        assert_eq!(reader.pending_bytes(), 0);
    }

    #[test]
    fn queue_rejects_empty_and_oversized_scans_without_replacing_active_scan() {
        let mut reader = BardigunBarcodeReader::new();
        reader.queue_scan(vec![0x12, 0x34]).unwrap();

        assert!(reader.queue_scan(Vec::new()).is_err());
        assert!(
            reader
                .queue_scan(vec![0; MAX_BARDIGUN_SCAN_BYTES + 1])
                .is_err()
        );
        assert_eq!(exchange(&mut reader, &[0xFF; 2]), [0x12, 0x34]);
    }

    #[test]
    fn queueing_another_valid_scan_aborts_and_restarts_the_active_scan() {
        let mut reader = BardigunBarcodeReader::new();
        reader.queue_scan(vec![0x10, 0x20, 0x30]).unwrap();
        assert_eq!(reader.exchange_byte(0xFF), 0x10);

        reader.queue_scan(vec![0xA0, 0xB0]).unwrap();

        assert_eq!(exchange(&mut reader, &[0xFF; 3]), [0xA0, 0xB0, 0]);
    }

    #[test]
    fn state_roundtrip_preserves_mid_scan_cursor() {
        let mut reader = BardigunBarcodeReader::new();
        reader.queue_scan(vec![0x10, 0x20, 0x30]).unwrap();
        assert_eq!(reader.exchange_byte(0xFF), 0x10);

        let mut writer = StateWriter::new();
        reader.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut state_reader = StateReader::new(&bytes);
        let mut restored = BardigunBarcodeReader::read_state(&mut state_reader).unwrap();

        assert!(state_reader.is_exhausted());
        assert_eq!(exchange(&mut restored, &[0xFF; 3]), [0x20, 0x30, 0]);
    }

    #[test]
    fn state_rejects_oversized_truncated_and_invalid_cursor_payloads() {
        let mut oversized = StateWriter::new();
        oversized.write_u64((MAX_BARDIGUN_SCAN_BYTES + 1) as u64);
        assert!(
            BardigunBarcodeReader::read_state(&mut StateReader::new(&oversized.into_bytes()))
                .is_err()
        );

        let mut truncated = StateWriter::new();
        truncated.write_u64(2);
        truncated.write_u8(0xAA);
        assert!(
            BardigunBarcodeReader::read_state(&mut StateReader::new(&truncated.into_bytes()))
                .is_err()
        );

        let mut bad_cursor = StateWriter::new();
        bad_cursor.write_u64(2);
        bad_cursor.write_bytes(&[0xAA, 0xBB]);
        bad_cursor.write_u64(2);
        assert!(
            BardigunBarcodeReader::read_state(&mut StateReader::new(&bad_cursor.into_bytes()))
                .is_err()
        );
    }
}
