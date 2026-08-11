use zeff_emu_common::save_state::{StateReader, StateWriter};

const EXT_DATA_UNUSED_BIT: u8 = 0x80;
const EXT_DATA_PIN_MASK: u8 = 0x7F;
const SERIAL_STATUS_TX_FULL: u8 = 0x01;
const SERIAL_STATUS_RX_READY: u8 = 0x02;
const SERIAL_STATUS_ERROR: u8 = 0x04;
const SERIAL_CONTROL_RW_MASK: u8 = 0xF8;
const SERIAL_CONTROL_TX_ENABLE: u8 = 0x10;
const SERIAL_CONTROL_RX_ENABLE: u8 = 0x20;
const SERIAL_CONTROL_LINK_ENABLE_MASK: u8 = SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE;
const SERIAL_STATE_FLAG_TX_PENDING: u8 = 0x01;
const SERIAL_STATE_FLAG_RX_READY: u8 = 0x02;
const DEFAULT_EXT_DATA: u8 = 0xFF;
const DEFAULT_EXT_DIRECTION: u8 = 0xFF;
const DEFAULT_SERIAL_DATA: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameGearSerialDebugSnapshot {
    pub ext_data: u8,
    pub ext_direction: u8,
    pub tx_data: u8,
    pub rx_data: u8,
    pub control: u8,
    pub status: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameGearSerial {
    ext_data: u8,
    ext_direction: u8,
    tx_data: u8,
    rx_data: u8,
    control: u8,
    tx_pending: bool,
    rx_ready: bool,
    peer_present: bool,
    peer_ext_data: u8,
    peer_ext_direction: u8,
}

impl GameGearSerial {
    pub fn new() -> Self {
        Self {
            ext_data: DEFAULT_EXT_DATA,
            ext_direction: DEFAULT_EXT_DIRECTION,
            tx_data: DEFAULT_SERIAL_DATA,
            rx_data: DEFAULT_SERIAL_DATA,
            control: 0,
            tx_pending: false,
            rx_ready: false,
            peer_present: false,
            peer_ext_data: DEFAULT_EXT_DATA,
            peer_ext_direction: DEFAULT_EXT_DIRECTION,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn read_ext_data(&self) -> u8 {
        let input_mask = self.ext_direction & EXT_DATA_PIN_MASK;
        let mut value = self.ext_data & !input_mask;
        for bit in 0..7 {
            let mask = 1 << bit;
            if input_mask & mask == 0 {
                continue;
            }
            if self.peer_present && self.peer_ext_direction & mask == 0 {
                value |= self.peer_ext_data & mask;
            } else {
                value |= mask;
            }
        }
        value | EXT_DATA_UNUSED_BIT
    }

    pub fn write_ext_data(&mut self, value: u8) {
        self.ext_data = value | EXT_DATA_UNUSED_BIT;
    }

    pub fn ext_direction(&self) -> u8 {
        self.ext_direction
    }

    pub fn write_ext_direction(&mut self, value: u8) {
        self.ext_direction = value;
    }

    pub fn tx_data(&self) -> u8 {
        self.tx_data
    }

    pub fn write_tx_data(&mut self, value: u8) {
        self.tx_data = value;
        if self.serial_operation_enabled() {
            self.tx_pending = true;
        }
    }

    pub fn rx_data(&self) -> u8 {
        self.rx_data
    }

    pub fn read_rx_data(&mut self) -> u8 {
        let value = self.rx_data;
        self.rx_ready = false;
        value
    }

    pub fn control(&self) -> u8 {
        self.control
    }

    pub fn write_control(&mut self, value: u8) {
        self.control = value & SERIAL_CONTROL_RW_MASK;
        if !self.serial_operation_enabled() {
            self.tx_pending = false;
            self.rx_ready = false;
        }
    }

    pub fn read_status(&self) -> u8 {
        let mut status = self.control & SERIAL_CONTROL_RW_MASK;
        if self.serial_operation_enabled() && !self.peer_present {
            status |= SERIAL_STATUS_ERROR;
        } else if self.serial_operation_enabled() {
            if self.tx_pending {
                status |= SERIAL_STATUS_TX_FULL;
            }
            if self.rx_ready {
                status |= SERIAL_STATUS_RX_READY;
            }
        }
        status
    }

    pub fn disconnect_peer(&mut self) {
        self.peer_present = false;
        self.peer_ext_data = DEFAULT_EXT_DATA;
        self.peer_ext_direction = DEFAULT_EXT_DIRECTION;
    }

    pub fn exchange_with_peer(&mut self, peer: &mut Self) {
        self.peer_present = true;
        peer.peer_present = true;
        self.peer_ext_data = peer.ext_data;
        self.peer_ext_direction = peer.ext_direction;
        peer.peer_ext_data = self.ext_data;
        peer.peer_ext_direction = self.ext_direction;

        let self_to_peer = self.tx_pending
            && self.tx_enabled()
            && peer.rx_enabled()
            && !peer.rx_ready
            && peer.serial_operation_enabled();
        let peer_to_self = peer.tx_pending
            && peer.tx_enabled()
            && self.rx_enabled()
            && !self.rx_ready
            && self.serial_operation_enabled();

        if self_to_peer {
            peer.rx_data = self.tx_data;
            peer.rx_ready = true;
            self.tx_pending = false;
        }

        if peer_to_self {
            self.rx_data = peer.tx_data;
            self.rx_ready = true;
            peer.tx_pending = false;
        }
    }

    pub fn debug_snapshot(&self) -> GameGearSerialDebugSnapshot {
        GameGearSerialDebugSnapshot {
            ext_data: self.read_ext_data(),
            ext_direction: self.ext_direction,
            tx_data: self.tx_data,
            rx_data: self.rx_data,
            control: self.control,
            status: self.read_status(),
        }
    }

    pub(crate) fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(self.ext_data);
        w.write_u8(self.ext_direction);
        w.write_u8(self.tx_data);
        w.write_u8(self.rx_data);
        w.write_u8(self.control);
        w.write_u8(self.state_flags());
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut StateReader<'_>,
        version_has_flags: bool,
    ) -> anyhow::Result<()> {
        self.ext_data = r.read_u8()?;
        self.ext_direction = r.read_u8()?;
        self.tx_data = r.read_u8()?;
        self.rx_data = r.read_u8()?;
        self.control = r.read_u8()? & SERIAL_CONTROL_RW_MASK;
        let flags = if version_has_flags { r.read_u8()? } else { 0 };
        self.tx_pending = flags & SERIAL_STATE_FLAG_TX_PENDING != 0;
        self.rx_ready = flags & SERIAL_STATE_FLAG_RX_READY != 0;
        self.disconnect_peer();
        Ok(())
    }

    fn serial_operation_enabled(&self) -> bool {
        self.control & SERIAL_CONTROL_LINK_ENABLE_MASK == SERIAL_CONTROL_LINK_ENABLE_MASK
    }

    fn tx_enabled(&self) -> bool {
        self.control & SERIAL_CONTROL_TX_ENABLE != 0
    }

    fn rx_enabled(&self) -> bool {
        self.control & SERIAL_CONTROL_RX_ENABLE != 0
    }

    fn state_flags(&self) -> u8 {
        let mut flags = 0;
        if self.tx_pending {
            flags |= SERIAL_STATE_FLAG_TX_PENDING;
        }
        if self.rx_ready {
            flags |= SERIAL_STATE_FLAG_RX_READY;
        }
        flags
    }
}

impl Default for GameGearSerial {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_is_idle_with_no_received_byte() {
        let serial = GameGearSerial::new();

        assert_eq!(serial.read_status() & SERIAL_STATUS_TX_FULL, 0);
        assert_eq!(serial.read_status() & SERIAL_STATUS_RX_READY, 0);
    }

    #[test]
    fn enabling_serial_without_peer_sets_error_but_keeps_tx_idle() {
        let mut serial = GameGearSerial::new();

        serial.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);

        assert_eq!(serial.read_status() & SERIAL_STATUS_TX_FULL, 0);
        assert_eq!(serial.read_status() & SERIAL_STATUS_RX_READY, 0);
        assert_ne!(serial.read_status() & SERIAL_STATUS_ERROR, 0);
    }

    #[test]
    fn serial_peer_exchange_transfers_tx_to_peer_rx() {
        let mut left = GameGearSerial::new();
        let mut right = GameGearSerial::new();
        left.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        right.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);

        left.write_tx_data(0x5A);
        left.exchange_with_peer(&mut right);

        assert_eq!(left.read_status() & SERIAL_STATUS_ERROR, 0);
        assert_eq!(left.read_status() & SERIAL_STATUS_TX_FULL, 0);
        assert_eq!(right.read_status() & SERIAL_STATUS_ERROR, 0);
        assert_ne!(right.read_status() & SERIAL_STATUS_RX_READY, 0);
        assert_eq!(right.read_rx_data(), 0x5A);
        assert_eq!(right.read_status() & SERIAL_STATUS_RX_READY, 0);
    }

    #[test]
    fn serial_peer_exchange_can_transfer_both_directions() {
        let mut left = GameGearSerial::new();
        let mut right = GameGearSerial::new();
        left.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        right.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);

        left.write_tx_data(0x12);
        right.write_tx_data(0x34);
        left.exchange_with_peer(&mut right);

        assert_eq!(left.read_rx_data(), 0x34);
        assert_eq!(right.read_rx_data(), 0x12);
    }

    #[test]
    fn ext_data_input_pins_read_high_when_disconnected() {
        let mut serial = GameGearSerial::new();

        serial.write_ext_direction(0x7F);
        serial.write_ext_data(0x00);

        assert_eq!(serial.read_ext_data(), 0xFF);
    }

    #[test]
    fn ext_data_output_pins_read_latched_value() {
        let mut serial = GameGearSerial::new();

        serial.write_ext_direction(0x00);
        serial.write_ext_data(0x2A);

        assert_eq!(serial.read_ext_data(), 0xAA);
    }

    #[test]
    fn ext_data_input_pins_can_read_connected_peer_outputs() {
        let mut left = GameGearSerial::new();
        let mut right = GameGearSerial::new();
        left.write_ext_direction(0x7F);
        right.write_ext_direction(0x00);
        right.write_ext_data(0x2A);

        left.exchange_with_peer(&mut right);

        assert_eq!(left.read_ext_data(), 0xAA);
    }

    #[test]
    fn pending_and_ready_flags_roundtrip_through_state() {
        let mut saved = GameGearSerial::new();
        let mut peer = GameGearSerial::new();
        saved.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        peer.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        peer.write_tx_data(0xC3);
        saved.exchange_with_peer(&mut peer);
        saved.write_tx_data(0x5A);

        let mut writer = StateWriter::with_capacity(16);
        saved.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut restored = GameGearSerial::new();
        restored
            .read_state(&mut reader, true)
            .expect("serial state should decode");

        assert_eq!(restored.read_rx_data(), 0xC3);
        restored.exchange_with_peer(&mut peer);
        assert_eq!(peer.read_rx_data(), 0x5A);
    }
}
