use super::serial::SerialDevice;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

pub const BARCODE_BOY_EAN_DIGITS: usize = 13;
pub const BARCODE_BOY_PAYLOAD_BYTES: usize = 30;

/// GBE+-compatible deterministic policy used while the guest continuously arms external clock.
pub const BARCODE_BOY_BYTE_PERIOD_T_CYCLES: u64 = 4096;

const HANDSHAKE_TX: [u8; 4] = [0x10, 0x07, 0x10, 0x07];
const HANDSHAKE_RX: [u8; 4] = [0xFF, 0xFF, 0x10, 0x07];

#[derive(Debug, Default)]
pub struct BarcodeBoy {
    handshake_position: u8,
    handshake_complete: bool,
    payload: Option<[u8; BARCODE_BOY_PAYLOAD_BYTES]>,
    payload_position: u8,
    external_clock_cycles: u64,
}

impl BarcodeBoy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger_scan(&mut self, digits: &str) -> Result<()> {
        validate_digits(digits)?;
        if !self.handshake_complete {
            bail!("Barcode Boy handshake is not complete");
        }
        if self.payload.is_some() {
            bail!("Barcode Boy scan is already in progress");
        }

        let mut payload = [0; BARCODE_BOY_PAYLOAD_BYTES];
        payload[0] = 0x02;
        payload[1..14].copy_from_slice(digits.as_bytes());
        payload[14] = 0x03;
        let (first, second) = payload.split_at_mut(15);
        second.copy_from_slice(first);
        self.payload = Some(payload);
        self.payload_position = 0;
        self.external_clock_cycles = 0;
        Ok(())
    }

    pub fn handshake_complete(&self) -> bool {
        self.handshake_complete
    }

    pub fn scan_pending(&self) -> bool {
        self.payload.is_some()
    }

    pub(crate) fn reconnect(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn clock_external(
        &mut self,
        t_cycles: u64,
        guest_external_clock_armed: bool,
    ) -> Option<u8> {
        if !guest_external_clock_armed || !self.handshake_complete || self.payload.is_none() {
            self.external_clock_cycles = 0;
            return None;
        }

        self.external_clock_cycles = self.external_clock_cycles.saturating_add(t_cycles);
        if self.external_clock_cycles < BARCODE_BOY_BYTE_PERIOD_T_CYCLES {
            return None;
        }
        self.external_clock_cycles = 0;

        let payload = self.payload.as_ref()?;
        let response = payload[usize::from(self.payload_position)];
        self.payload_position += 1;
        if usize::from(self.payload_position) == BARCODE_BOY_PAYLOAD_BYTES {
            self.reconnect();
        }
        Some(response)
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.handshake_position);
        writer.write_bool(self.handshake_complete);
        writer.write_bool(self.payload.is_some());
        if let Some(payload) = self.payload {
            writer.write_bytes(&payload);
        }
        writer.write_u8(self.payload_position);
        writer.write_u64(self.external_clock_cycles);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let handshake_position = reader.read_u8()?;
        let handshake_complete = reader.read_bool()?;
        if handshake_position > 3 {
            bail!("invalid Barcode Boy handshake position: {handshake_position}");
        }
        if handshake_complete && handshake_position != 0 {
            bail!("completed Barcode Boy handshake has a partial handshake position");
        }

        let payload = if reader.read_bool()? {
            let mut payload = [0; BARCODE_BOY_PAYLOAD_BYTES];
            reader.read_exact(&mut payload)?;
            validate_payload(&payload)?;
            Some(payload)
        } else {
            None
        };
        let payload_position = reader.read_u8()?;
        let external_clock_cycles = reader.read_u64()?;
        if payload.is_some() {
            if !handshake_complete {
                bail!("Barcode Boy payload exists before handshake completion");
            }
            if usize::from(payload_position) >= BARCODE_BOY_PAYLOAD_BYTES {
                bail!("invalid Barcode Boy payload position: {payload_position}");
            }
        } else if payload_position != 0 || external_clock_cycles != 0 {
            bail!("idle Barcode Boy has active payload timing state");
        }
        if external_clock_cycles >= BARCODE_BOY_BYTE_PERIOD_T_CYCLES {
            bail!("invalid Barcode Boy external clock phase: {external_clock_cycles}");
        }

        Ok(Self {
            handshake_position,
            handshake_complete,
            payload,
            payload_position,
            external_clock_cycles,
        })
    }
}

impl SerialDevice for BarcodeBoy {
    fn exchange_byte(&mut self, byte: u8) -> u8 {
        if self.handshake_complete {
            return 0xFF;
        }

        let position = usize::from(self.handshake_position);
        if byte == HANDSHAKE_TX[position] {
            let response = HANDSHAKE_RX[position];
            self.handshake_position += 1;
            if usize::from(self.handshake_position) == HANDSHAKE_TX.len() {
                self.handshake_position = 0;
                self.handshake_complete = true;
            }
            return response;
        }

        self.handshake_position = 0;
        0xFF
    }
}

fn validate_digits(digits: &str) -> Result<()> {
    if digits.len() != BARCODE_BOY_EAN_DIGITS || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("Barcode Boy scan must contain exactly 13 decimal digits");
    }
    Ok(())
}

fn validate_payload(payload: &[u8; BARCODE_BOY_PAYLOAD_BYTES]) -> Result<()> {
    if payload[0] != 0x02
        || payload[14] != 0x03
        || payload[..15] != payload[15..]
        || !payload[1..14].iter().all(u8::is_ascii_digit)
    {
        bail!("invalid Barcode Boy payload in save state");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake(device: &mut BarcodeBoy) -> Vec<u8> {
        HANDSHAKE_TX
            .into_iter()
            .map(|byte| device.exchange_byte(byte))
            .collect()
    }

    #[test]
    fn exact_handshake_transcript_arms_external_clock_mode() {
        let mut device = BarcodeBoy::new();
        assert_eq!(handshake(&mut device), HANDSHAKE_RX);
        assert!(device.handshake_complete());
    }

    #[test]
    fn handshake_mismatch_resets_detection() {
        let mut device = BarcodeBoy::new();
        assert_eq!(device.exchange_byte(0x10), 0xFF);
        assert_eq!(device.exchange_byte(0x10), 0xFF);
        assert_eq!(device.exchange_byte(0x07), 0xFF);
        assert_eq!(device.exchange_byte(0x10), 0xFF);
        assert_eq!(device.exchange_byte(0x00), 0xFF);
        assert!(!device.handshake_complete());

        assert_eq!(handshake(&mut device), HANDSHAKE_RX);
        assert!(device.handshake_complete());
    }

    #[test]
    fn triggered_ean_is_framed_and_repeated_without_auto_scan() {
        let mut device = BarcodeBoy::new();
        handshake(&mut device);
        device.trigger_scan("1234567890123").unwrap();
        assert!(device.scan_pending());
        let mut bytes = Vec::new();
        for _ in 0..BARCODE_BOY_PAYLOAD_BYTES {
            assert_eq!(
                device.clock_external(BARCODE_BOY_BYTE_PERIOD_T_CYCLES - 1, true),
                None
            );
            bytes.push(device.clock_external(1, true).unwrap());
        }

        let expected = b"\x021234567890123\x03";
        assert_eq!(&bytes[..15], expected);
        assert_eq!(&bytes[15..], expected);
        assert!(!device.scan_pending());
        assert!(!device.handshake_complete());
        assert_eq!(device.clock_external(100_000, true), None);
    }

    #[test]
    fn trigger_requires_exact_decimal_ean_and_rejects_overlap() {
        let mut device = BarcodeBoy::new();
        for invalid in ["", "123456789012", "12345678901234", "123456789012X"] {
            assert!(device.trigger_scan(invalid).is_err());
        }
        assert!(device.trigger_scan("0000000000000").is_err());
        handshake(&mut device);
        device.trigger_scan("0000000000000").unwrap();
        assert!(device.trigger_scan("1234567890123").is_err());
    }

    #[test]
    fn external_clock_phase_requires_continuous_guest_arming() {
        let mut device = BarcodeBoy::new();
        handshake(&mut device);
        device.trigger_scan("1234567890123").unwrap();
        assert_eq!(device.clock_external(2048, true), None);
        assert_eq!(device.clock_external(1, false), None);
        assert_eq!(device.clock_external(2048, true), None);
        assert_eq!(device.clock_external(2048, true), Some(0x02));
    }

    #[test]
    fn state_roundtrip_preserves_handshake_payload_cursor_and_clock_phase() {
        let mut device = BarcodeBoy::new();
        handshake(&mut device);
        device.trigger_scan("1234567890123").unwrap();
        assert_eq!(
            device.clock_external(BARCODE_BOY_BYTE_PERIOD_T_CYCLES, true),
            Some(0x02)
        );
        assert_eq!(device.clock_external(1234, true), None);

        let mut writer = StateWriter::new();
        device.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut restored = BarcodeBoy::read_state(&mut reader).unwrap();

        assert!(reader.is_exhausted());
        assert_eq!(
            restored.clock_external(BARCODE_BOY_BYTE_PERIOD_T_CYCLES - 1234, true),
            Some(b'1')
        );
    }

    #[test]
    fn malformed_state_is_rejected() {
        let mut bad_handshake = StateWriter::new();
        bad_handshake.write_u8(4);
        bad_handshake.write_bool(false);
        bad_handshake.write_bool(false);
        bad_handshake.write_u8(0);
        bad_handshake.write_u64(0);
        assert!(
            BarcodeBoy::read_state(&mut StateReader::new(&bad_handshake.into_bytes())).is_err()
        );

        let mut bad_payload = StateWriter::new();
        bad_payload.write_u8(0);
        bad_payload.write_bool(false);
        bad_payload.write_bool(true);
        let mut valid_payload = [0; BARCODE_BOY_PAYLOAD_BYTES];
        valid_payload[..15].copy_from_slice(b"\x021234567890123\x03");
        valid_payload[15..].copy_from_slice(b"\x021234567890123\x03");
        bad_payload.write_bytes(&valid_payload);
        bad_payload.write_u8(0);
        bad_payload.write_u64(0);
        let error = BarcodeBoy::read_state(&mut StateReader::new(&bad_payload.into_bytes()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("before handshake completion"));
    }
}
