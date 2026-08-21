use super::{DisconnectedDevice, SerialDevice};
use crate::hardware::barcode_boy::BarcodeBoy;
use crate::hardware::bardigun::BardigunBarcodeReader;
use crate::hardware::printer::GameboyPrinter;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GameBoySerialDevice {
    #[default]
    Disconnected,
    Printer,
    BardigunBarcodeReader,
    BarcodeBoy,
}

#[derive(Debug)]
pub(crate) struct SerialDevicePort {
    selected: GameBoySerialDevice,
    disconnected: DisconnectedDevice,
    printer: GameboyPrinter,
    bardigun_barcode_reader: BardigunBarcodeReader,
    barcode_boy: BarcodeBoy,
}

impl SerialDevicePort {
    pub(crate) fn new() -> Self {
        Self {
            selected: GameBoySerialDevice::Disconnected,
            disconnected: DisconnectedDevice,
            printer: GameboyPrinter::new(),
            bardigun_barcode_reader: BardigunBarcodeReader::new(),
            barcode_boy: BarcodeBoy::new(),
        }
    }

    pub(crate) fn selected(&self) -> GameBoySerialDevice {
        self.selected
    }

    pub(crate) fn select(&mut self, selected: GameBoySerialDevice) {
        if selected == GameBoySerialDevice::Printer && self.selected != selected {
            self.printer.reconnect();
        }
        if selected == GameBoySerialDevice::BardigunBarcodeReader && self.selected != selected {
            self.bardigun_barcode_reader.reconnect();
        }
        if selected == GameBoySerialDevice::BarcodeBoy && self.selected != selected {
            self.barcode_boy.reconnect();
        }
        self.selected = selected;
    }

    pub(crate) fn printer(&self) -> &GameboyPrinter {
        &self.printer
    }

    pub(crate) fn printer_mut(&mut self) -> &mut GameboyPrinter {
        &mut self.printer
    }

    pub(crate) fn queue_bardigun_barcode_scan(&mut self, scan: Vec<u8>) -> Result<()> {
        if self.selected != GameBoySerialDevice::BardigunBarcodeReader {
            bail!("Bardigun Barcode Reader is not attached");
        }
        self.bardigun_barcode_reader.queue_scan(scan)
    }

    #[cfg(test)]
    pub(crate) fn bardigun_pending_bytes(&self) -> usize {
        self.bardigun_barcode_reader.pending_bytes()
    }

    pub(crate) fn trigger_barcode_boy_scan(&mut self, digits: &str) -> Result<()> {
        if self.selected != GameBoySerialDevice::BarcodeBoy {
            bail!("Barcode Boy is not attached");
        }
        self.barcode_boy.trigger_scan(digits)
    }

    pub(crate) fn clock_active_external_device(
        &mut self,
        t_cycles: u64,
        guest_external_clock_armed: bool,
    ) -> Option<u8> {
        match self.selected {
            GameBoySerialDevice::BarcodeBoy => self
                .barcode_boy
                .clock_external(t_cycles, guest_external_clock_armed),
            _ => None,
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(match self.selected {
            GameBoySerialDevice::Disconnected => 0,
            GameBoySerialDevice::Printer => 1,
            GameBoySerialDevice::BardigunBarcodeReader => 2,
            GameBoySerialDevice::BarcodeBoy => 3,
        });
        self.printer.write_state(writer);
        self.bardigun_barcode_reader.write_state(writer);
        self.barcode_boy.write_state(writer);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>, format_version: u32) -> Result<Self> {
        let selected = if format_version >= 6 {
            match reader.read_u8()? {
                0 => GameBoySerialDevice::Disconnected,
                1 => GameBoySerialDevice::Printer,
                2 if format_version >= 7 => GameBoySerialDevice::BardigunBarcodeReader,
                3 if format_version >= 8 => GameBoySerialDevice::BarcodeBoy,
                tag => bail!("invalid Game Boy serial device tag in save state: {tag}"),
            }
        } else {
            GameBoySerialDevice::Printer
        };

        let printer = if format_version >= 6 {
            GameboyPrinter::read_state(reader, format_version)?
        } else {
            GameboyPrinter::read_legacy_state(reader)?
        };
        let bardigun_barcode_reader = if format_version >= 7 {
            BardigunBarcodeReader::read_state(reader)?
        } else {
            BardigunBarcodeReader::new()
        };
        let barcode_boy = if format_version >= 8 {
            BarcodeBoy::read_state(reader)?
        } else {
            BarcodeBoy::new()
        };

        Ok(Self {
            selected,
            disconnected: DisconnectedDevice,
            printer,
            bardigun_barcode_reader,
            barcode_boy,
        })
    }
}

impl SerialDevice for SerialDevicePort {
    fn exchange_byte(&mut self, byte: u8) -> u8 {
        match self.selected {
            GameBoySerialDevice::Disconnected => self.disconnected.exchange_byte(byte),
            GameBoySerialDevice::Printer => self.printer.exchange_byte(byte),
            GameBoySerialDevice::BardigunBarcodeReader => {
                self.bardigun_barcode_reader.exchange_byte(byte)
            }
            GameBoySerialDevice::BarcodeBoy => self.barcode_boy.exchange_byte(byte),
        }
    }

    fn step(&mut self, t_cycles: u64) {
        if self.selected == GameBoySerialDevice::Printer {
            self.printer.step(t_cycles);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(command: u8, payload: &[u8]) -> Vec<u8> {
        let len = u16::try_from(payload.len()).unwrap();
        let mut bytes = vec![0x88, 0x33, command, 0, len as u8, (len >> 8) as u8];
        bytes.extend_from_slice(payload);
        let checksum = bytes[2..]
            .iter()
            .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
        bytes.extend_from_slice(&checksum.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    fn exchange(port: &mut SerialDevicePort, bytes: &[u8]) -> Vec<u8> {
        bytes.iter().map(|byte| port.exchange_byte(*byte)).collect()
    }

    #[test]
    fn format_nine_roundtrip_preserves_selected_device() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::Printer);
        let mut writer = StateWriter::new();
        port.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let restored = SerialDevicePort::read_state(&mut reader, 9).unwrap();

        assert_eq!(restored.selected(), GameBoySerialDevice::Printer);
        assert!(reader.is_exhausted());
    }

    #[test]
    fn format_nine_roundtrip_preserves_bardigun_mid_scan() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BardigunBarcodeReader);
        port.queue_bardigun_barcode_scan(vec![0x10, 0x20, 0x30])
            .unwrap();
        assert_eq!(port.exchange_byte(0xFF), 0x10);
        let mut writer = StateWriter::new();
        port.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let mut restored = SerialDevicePort::read_state(&mut reader, 9).unwrap();

        assert_eq!(
            restored.selected(),
            GameBoySerialDevice::BardigunBarcodeReader
        );
        assert_eq!(exchange(&mut restored, &[0xFF; 3]), [0x20, 0x30, 0]);
        assert!(reader.is_exhausted());
    }

    #[test]
    fn format_seven_state_defaults_barcode_boy_to_idle() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BardigunBarcodeReader);
        port.queue_bardigun_barcode_scan(vec![0x10, 0x20]).unwrap();
        let mut writer = StateWriter::new();
        writer.write_u8(2);
        port.printer.write_legacy_current_state(&mut writer);
        port.bardigun_barcode_reader.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let mut restored = SerialDevicePort::read_state(&mut reader, 7).unwrap();

        assert_eq!(
            restored.selected(),
            GameBoySerialDevice::BardigunBarcodeReader
        );
        restored.select(GameBoySerialDevice::BarcodeBoy);
        assert!(restored.trigger_barcode_boy_scan("1234567890123").is_err());
        assert!(reader.is_exhausted());
    }

    #[test]
    fn format_nine_roundtrip_preserves_barcode_boy_mid_scan() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BarcodeBoy);
        assert_eq!(
            exchange(&mut port, &[0x10, 0x07, 0x10, 0x07]),
            [0xFF, 0xFF, 0x10, 0x07]
        );
        port.trigger_barcode_boy_scan("1234567890123").unwrap();
        assert_eq!(
            port.clock_active_external_device(
                crate::hardware::barcode_boy::BARCODE_BOY_BYTE_PERIOD_T_CYCLES,
                true,
            ),
            Some(0x02)
        );
        assert_eq!(port.clock_active_external_device(1234, true), None);
        let mut writer = StateWriter::new();
        port.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let mut restored = SerialDevicePort::read_state(&mut reader, 9).unwrap();

        assert_eq!(restored.selected(), GameBoySerialDevice::BarcodeBoy);
        assert_eq!(
            restored.clock_active_external_device(
                crate::hardware::barcode_boy::BARCODE_BOY_BYTE_PERIOD_T_CYCLES - 1234,
                true,
            ),
            Some(b'1')
        );
        assert!(reader.is_exhausted());
    }

    #[test]
    fn format_eight_decodes_legacy_printer_and_current_accessories() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BarcodeBoy);
        assert_eq!(
            exchange(&mut port, &[0x10, 0x07, 0x10, 0x07]),
            [0xFF, 0xFF, 0x10, 0x07]
        );
        let mut writer = StateWriter::new();
        writer.write_u8(3);
        port.printer.write_legacy_current_state(&mut writer);
        port.bardigun_barcode_reader.write_state(&mut writer);
        port.barcode_boy.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let mut restored = SerialDevicePort::read_state(&mut reader, 8).unwrap();

        assert!(reader.is_exhausted());
        assert_eq!(restored.selected(), GameBoySerialDevice::BarcodeBoy);
        restored.trigger_barcode_boy_scan("1234567890123").unwrap();
    }

    #[test]
    fn format_six_state_defaults_bardigun_to_idle_without_consuming_extra_bytes() {
        let port = SerialDevicePort::new();
        let mut writer = StateWriter::new();
        writer.write_u8(1);
        port.printer.write_legacy_current_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let mut restored = SerialDevicePort::read_state(&mut reader, 6).unwrap();

        assert_eq!(restored.selected(), GameBoySerialDevice::Printer);
        restored.select(GameBoySerialDevice::BardigunBarcodeReader);
        assert_eq!(restored.exchange_byte(0xFF), 0);
        assert!(reader.is_exhausted());
    }

    #[test]
    fn format_six_rejects_unknown_device_tag() {
        let bytes = [2];
        let mut reader = StateReader::new(&bytes);
        let error = SerialDevicePort::read_state(&mut reader, 6)
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid Game Boy serial device tag"));
    }

    #[test]
    fn reconnecting_printer_resets_guest_protocol_but_keeps_completed_jobs() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::Printer);
        exchange(&mut port, &packet(0x04, &[0; 16]));
        exchange(&mut port, &packet(0x04, &[]));
        exchange(&mut port, &packet(0x02, &[1, 0, 0xE4, 0x40]));
        assert_eq!(port.printer.job_count(), 1);

        exchange(&mut port, &[0x88, 0x33, 0x04]);
        port.select(GameBoySerialDevice::Disconnected);

        port.select(GameBoySerialDevice::Printer);

        let replies = exchange(&mut port, &packet(0x01, &[]));
        assert_eq!(&replies[replies.len() - 2..], &[0x81, 0]);
        assert_eq!(port.printer.job_count(), 1);
    }

    #[test]
    fn reconnecting_bardigun_reader_discards_an_incomplete_scan() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BardigunBarcodeReader);
        port.queue_bardigun_barcode_scan(vec![0x12, 0x34]).unwrap();
        assert_eq!(port.exchange_byte(0xFF), 0x12);

        port.select(GameBoySerialDevice::Disconnected);
        port.select(GameBoySerialDevice::BardigunBarcodeReader);

        assert_eq!(port.exchange_byte(0xFF), 0);
    }

    #[test]
    fn bardigun_scan_requires_the_reader_to_be_attached() {
        let mut port = SerialDevicePort::new();
        assert!(
            port.queue_bardigun_barcode_scan(vec![0x12])
                .unwrap_err()
                .to_string()
                .contains("not attached")
        );

        port.select(GameBoySerialDevice::Printer);
        assert!(port.queue_bardigun_barcode_scan(vec![0x12]).is_err());

        port.select(GameBoySerialDevice::BardigunBarcodeReader);
        port.queue_bardigun_barcode_scan(vec![0x12, 0x34]).unwrap();
        assert_eq!(exchange(&mut port, &[0xFF; 2]), [0x12, 0x34]);
    }

    #[test]
    fn reconnecting_barcode_boy_requires_a_fresh_handshake() {
        let mut port = SerialDevicePort::new();
        port.select(GameBoySerialDevice::BarcodeBoy);
        assert_eq!(
            exchange(&mut port, &[0x10, 0x07, 0x10, 0x07]),
            [0xFF, 0xFF, 0x10, 0x07]
        );
        port.trigger_barcode_boy_scan("1234567890123").unwrap();

        port.select(GameBoySerialDevice::Disconnected);
        port.select(GameBoySerialDevice::BarcodeBoy);

        assert!(port.trigger_barcode_boy_scan("1234567890123").is_err());
    }

    #[test]
    fn legacy_state_restores_historical_printer_attachment() {
        let mut writer = StateWriter::new();
        writer.write_u8(0);
        writer.write_bytes(&[0; 5]);
        writer.write_u64(0);
        writer.write_u64(0);
        writer.write_u64(0);
        writer.write_u8(0x08);
        writer.write_u64(0);
        let bytes = writer.into_bytes();

        let mut reader = StateReader::new(&bytes);
        let restored = SerialDevicePort::read_state(&mut reader, 5).unwrap();

        assert_eq!(restored.selected(), GameBoySerialDevice::Printer);
        assert!(reader.is_exhausted());
    }
}
