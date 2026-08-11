use zeff_emu_common::save_state::{StateReader, StateWriter};

const EXT_DATA_UNUSED_BIT: u8 = 0x80;
const EXT_DATA_PIN_MASK: u8 = 0x7F;
const SERIAL_STATUS_TX_FULL: u8 = 0x01;
const SERIAL_STATUS_RX_READY: u8 = 0x02;
const SERIAL_STATUS_ERROR: u8 = 0x04;
const SERIAL_CONTROL_RW_MASK: u8 = 0xF8;
const SERIAL_CONTROL_RX_NMI_ENABLE: u8 = 0x08;
const SERIAL_CONTROL_TX_ENABLE: u8 = 0x10;
const SERIAL_CONTROL_RX_ENABLE: u8 = 0x20;
const SERIAL_CONTROL_LINK_ENABLE_MASK: u8 = SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE;
const SERIAL_CONTROL_BAUD_MASK: u8 = 0xC0;
const SERIAL_STATE_FLAG_TX_PENDING: u8 = 0x01;
const SERIAL_STATE_FLAG_RX_READY: u8 = 0x02;
const SERIAL_STATE_FLAG_TX_DELIVERY_READY: u8 = 0x04;
const SERIAL_STATE_FLAG_RX_ERROR: u8 = 0x08;
const SERIAL_STATE_FLAG_RX_NMI_PENDING: u8 = 0x10;
const SERIAL_FRAME_BITS: u64 = 10;
const DEFAULT_EXT_DATA: u8 = 0xFF;
const DEFAULT_EXT_DIRECTION: u8 = 0xFF;
const DEFAULT_SERIAL_DATA: u8 = 0xFF;
const EXT_PEER_OUTPUT_MASK_BY_INPUT_BIT: [u8; 7] =
    [1 << 2, 1 << 3, 1 << 0, 1 << 1, 1 << 5, 1 << 4, 1 << 6];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameGearSerialDebugSnapshot {
    pub ext_data: u8,
    pub ext_direction: u8,
    pub tx_data: u8,
    pub rx_data: u8,
    pub control: u8,
    pub status: u8,
    pub baud_bps: u32,
    pub tx_cycles_remaining: u32,
    pub rx_nmi_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameGearSerial {
    ext_data: u8,
    ext_direction: u8,
    tx_data: u8,
    rx_data: u8,
    control: u8,
    tx_pending: bool,
    tx_cycles_remaining: u32,
    tx_delivery_ready: bool,
    rx_ready: bool,
    rx_error: bool,
    rx_nmi_pending: bool,
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
            tx_cycles_remaining: 0,
            tx_delivery_ready: false,
            rx_ready: false,
            rx_error: false,
            rx_nmi_pending: false,
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
            let peer_mask = EXT_PEER_OUTPUT_MASK_BY_INPUT_BIT[bit as usize];
            if self.peer_present && self.peer_ext_direction & peer_mask == 0 {
                if self.peer_ext_data & peer_mask != 0 {
                    value |= mask;
                }
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
            self.tx_cycles_remaining = 0;
            self.tx_delivery_ready = false;
        }
    }

    pub fn rx_data(&self) -> u8 {
        self.rx_data
    }

    pub fn read_rx_data(&mut self) -> u8 {
        let value = self.rx_data;
        self.rx_ready = false;
        self.rx_error = false;
        self.rx_nmi_pending = false;
        value
    }

    pub fn control(&self) -> u8 {
        self.control
    }

    pub fn write_control(&mut self, value: u8) {
        self.control = value & SERIAL_CONTROL_RW_MASK;
        if !self.serial_operation_enabled() {
            self.tx_pending = false;
            self.tx_cycles_remaining = 0;
            self.tx_delivery_ready = false;
            self.rx_ready = false;
            self.rx_error = false;
            self.rx_nmi_pending = false;
        } else if self.control & SERIAL_CONTROL_RX_NMI_ENABLE == 0 {
            self.rx_nmi_pending = false;
        }
    }

    pub fn read_status(&self) -> u8 {
        let mut status = self.control & SERIAL_CONTROL_RW_MASK;
        if self.serial_operation_enabled() && !self.peer_present {
            status |= SERIAL_STATUS_ERROR;
        } else if self.serial_operation_enabled() {
            if self.rx_error {
                status |= SERIAL_STATUS_ERROR;
            }
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

    pub fn step_cycles(&mut self, cycles: u32, clock_hz: u32) {
        if !self.tx_pending || self.tx_delivery_ready {
            return;
        }
        self.ensure_tx_timer_started(clock_hz);
        if cycles >= self.tx_cycles_remaining {
            self.tx_cycles_remaining = 0;
            self.tx_delivery_ready = true;
        } else {
            self.tx_cycles_remaining -= cycles;
        }
    }

    pub fn baud_bps(&self) -> u32 {
        match self.control & SERIAL_CONTROL_BAUD_MASK {
            0x00 => 4_800,
            0x40 => 2_400,
            0x80 => 1_200,
            0xC0 => 300,
            _ => unreachable!("baud mask keeps only bits 6-7"),
        }
    }

    pub fn rx_nmi_pending(&self) -> bool {
        self.rx_nmi_pending
    }

    pub(crate) fn take_rx_nmi_pending(&mut self) -> bool {
        let pending = self.rx_nmi_pending;
        self.rx_nmi_pending = false;
        pending
    }

    pub fn exchange_with_peer(&mut self, peer: &mut Self, clock_hz: u32) {
        self.peer_present = true;
        peer.peer_present = true;
        self.peer_ext_data = peer.ext_data;
        self.peer_ext_direction = peer.ext_direction;
        peer.peer_ext_data = self.ext_data;
        peer.peer_ext_direction = self.ext_direction;

        self.ensure_tx_timer_started(clock_hz);
        peer.ensure_tx_timer_started(clock_hz);

        let self_to_peer = self.tx_delivery_ready
            && self.tx_enabled()
            && peer.rx_enabled()
            && self.serial_operation_enabled()
            && peer.serial_operation_enabled();
        let peer_to_self = peer.tx_delivery_ready
            && peer.tx_enabled()
            && self.rx_enabled()
            && peer.serial_operation_enabled()
            && self.serial_operation_enabled();

        if self_to_peer {
            peer.receive_byte(self.tx_data);
            self.finish_tx_delivery();
        }

        if peer_to_self {
            self.receive_byte(peer.tx_data);
            peer.finish_tx_delivery();
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
            baud_bps: self.baud_bps(),
            tx_cycles_remaining: self.tx_cycles_remaining,
            rx_nmi_pending: self.rx_nmi_pending(),
        }
    }

    pub(crate) fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(self.ext_data);
        w.write_u8(self.ext_direction);
        w.write_u8(self.tx_data);
        w.write_u8(self.rx_data);
        w.write_u8(self.control);
        w.write_u8(self.state_flags());
        w.write_u32(self.tx_cycles_remaining);
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut StateReader<'_>,
        version_has_flags: bool,
        version_has_timing: bool,
    ) -> anyhow::Result<()> {
        self.ext_data = r.read_u8()?;
        self.ext_direction = r.read_u8()?;
        self.tx_data = r.read_u8()?;
        self.rx_data = r.read_u8()?;
        self.control = r.read_u8()? & SERIAL_CONTROL_RW_MASK;
        let flags = if version_has_flags { r.read_u8()? } else { 0 };
        self.tx_pending = flags & SERIAL_STATE_FLAG_TX_PENDING != 0;
        self.rx_ready = flags & SERIAL_STATE_FLAG_RX_READY != 0;
        self.tx_delivery_ready = flags & SERIAL_STATE_FLAG_TX_DELIVERY_READY != 0;
        self.rx_error = flags & SERIAL_STATE_FLAG_RX_ERROR != 0;
        self.rx_nmi_pending = flags & SERIAL_STATE_FLAG_RX_NMI_PENDING != 0;
        self.tx_cycles_remaining = if version_has_timing { r.read_u32()? } else { 0 };
        if !version_has_timing && self.tx_pending {
            self.tx_delivery_ready = true;
        }
        self.sanitize_transfer_state();
        self.disconnect_peer();
        Ok(())
    }

    fn ensure_tx_timer_started(&mut self, clock_hz: u32) {
        if self.tx_pending && !self.tx_delivery_ready && self.tx_cycles_remaining == 0 {
            self.tx_cycles_remaining = Self::frame_cycles(clock_hz, self.baud_bps());
        }
    }

    fn frame_cycles(clock_hz: u32, baud_bps: u32) -> u32 {
        let cycles = (u64::from(clock_hz) * SERIAL_FRAME_BITS).div_ceil(u64::from(baud_bps.max(1)));
        cycles.min(u64::from(u32::MAX)) as u32
    }

    fn finish_tx_delivery(&mut self) {
        self.tx_pending = false;
        self.tx_cycles_remaining = 0;
        self.tx_delivery_ready = false;
    }

    fn receive_byte(&mut self, value: u8) {
        if self.rx_ready {
            self.rx_error = true;
        } else {
            self.rx_data = value;
            self.rx_ready = true;
            if self.control & SERIAL_CONTROL_RX_NMI_ENABLE != 0 {
                self.rx_nmi_pending = true;
            }
        }
    }

    fn sanitize_transfer_state(&mut self) {
        if !self.tx_pending {
            self.tx_cycles_remaining = 0;
            self.tx_delivery_ready = false;
        }
        if self.tx_delivery_ready {
            self.tx_cycles_remaining = 0;
        }
        if !self.rx_ready {
            self.rx_nmi_pending = false;
        }
        if !self.serial_operation_enabled() {
            self.tx_pending = false;
            self.tx_cycles_remaining = 0;
            self.tx_delivery_ready = false;
            self.rx_ready = false;
            self.rx_error = false;
            self.rx_nmi_pending = false;
        }
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
        if self.tx_delivery_ready {
            flags |= SERIAL_STATE_FLAG_TX_DELIVERY_READY;
        }
        if self.rx_error {
            flags |= SERIAL_STATE_FLAG_RX_ERROR;
        }
        if self.rx_nmi_pending {
            flags |= SERIAL_STATE_FLAG_RX_NMI_PENDING;
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

    const TEST_CLOCK_HZ: u32 = 3_579_545;

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
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert_ne!(left.read_status() & SERIAL_STATUS_TX_FULL, 0);
        assert_eq!(right.read_status() & SERIAL_STATUS_RX_READY, 0);

        let frame_cycles = GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800);
        left.step_cycles(frame_cycles - 1, TEST_CLOCK_HZ);
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert_ne!(left.read_status() & SERIAL_STATUS_TX_FULL, 0);
        assert_eq!(right.read_status() & SERIAL_STATUS_RX_READY, 0);

        left.step_cycles(1, TEST_CLOCK_HZ);
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

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
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);
        left.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        right.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert_eq!(left.read_rx_data(), 0x34);
        assert_eq!(right.read_rx_data(), 0x12);
    }

    #[test]
    fn baud_control_bits_select_uart_frame_duration() {
        for (bits, expected_baud) in [(0x00, 4_800), (0x40, 2_400), (0x80, 1_200), (0xC0, 300)] {
            let mut serial = GameGearSerial::new();
            serial.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE | bits);
            serial.write_tx_data(0x5A);
            serial.step_cycles(1, TEST_CLOCK_HZ);

            assert_eq!(serial.baud_bps(), expected_baud);
            assert_eq!(
                serial.tx_cycles_remaining,
                GameGearSerial::frame_cycles(TEST_CLOCK_HZ, expected_baud) - 1,
            );
        }
    }

    #[test]
    fn receive_nmi_pending_tracks_control_bit_and_clears_on_rx_read() {
        let mut left = GameGearSerial::new();
        let mut right = GameGearSerial::new();
        left.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        right.write_control(
            SERIAL_CONTROL_RX_NMI_ENABLE | SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE,
        );

        left.write_tx_data(0x5A);
        left.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert!(right.rx_nmi_pending());
        assert_eq!(right.read_rx_data(), 0x5A);
        assert!(!right.rx_nmi_pending());

        left.write_tx_data(0xA5);
        left.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        right.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert!(!right.rx_nmi_pending());
        assert_eq!(right.read_rx_data(), 0xA5);
    }

    #[test]
    fn receive_overrun_sets_error_until_rx_read() {
        let mut left = GameGearSerial::new();
        let mut right = GameGearSerial::new();
        left.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        right.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);

        left.write_tx_data(0x12);
        left.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);
        assert_ne!(right.read_status() & SERIAL_STATUS_RX_READY, 0);

        left.write_tx_data(0x34);
        left.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert_ne!(right.read_status() & SERIAL_STATUS_ERROR, 0);
        assert_eq!(right.read_rx_data(), 0x12);
        assert_eq!(right.read_status() & SERIAL_STATUS_ERROR, 0);
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

        left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

        assert_eq!(left.read_ext_data(), 0x9A);
    }

    #[test]
    fn ext_data_parallel_link_uses_game_gear_cross_wiring() {
        let cases = [(0, 2), (1, 3), (2, 0), (3, 1), (4, 5), (5, 4), (6, 6)];

        for (sent_bit, received_bit) in cases {
            let mut left = GameGearSerial::new();
            let mut right = GameGearSerial::new();
            left.write_ext_direction(0x7F);
            right.write_ext_direction(0x00);
            right.write_ext_data(1 << sent_bit);

            left.exchange_with_peer(&mut right, TEST_CLOCK_HZ);

            assert_eq!(
                left.read_ext_data(),
                EXT_DATA_UNUSED_BIT | (1 << received_bit),
                "peer output bit {sent_bit} should arrive on local input bit {received_bit}",
            );
        }
    }

    #[test]
    fn pending_and_ready_flags_roundtrip_through_state() {
        let mut saved = GameGearSerial::new();
        let mut peer = GameGearSerial::new();
        saved.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        peer.write_control(SERIAL_CONTROL_TX_ENABLE | SERIAL_CONTROL_RX_ENABLE);
        peer.write_tx_data(0xC3);
        peer.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        saved.exchange_with_peer(&mut peer, TEST_CLOCK_HZ);
        saved.write_tx_data(0x5A);
        saved.exchange_with_peer(&mut peer, TEST_CLOCK_HZ);
        saved.step_cycles(32, TEST_CLOCK_HZ);

        let mut writer = StateWriter::with_capacity(16);
        saved.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut restored = GameGearSerial::new();
        restored
            .read_state(&mut reader, true, true)
            .expect("serial state should decode");

        restored.exchange_with_peer(&mut peer, TEST_CLOCK_HZ);
        assert_eq!(restored.read_rx_data(), 0xC3);
        assert_ne!(restored.read_status() & SERIAL_STATUS_TX_FULL, 0);
        restored.step_cycles(
            GameGearSerial::frame_cycles(TEST_CLOCK_HZ, 4_800),
            TEST_CLOCK_HZ,
        );
        restored.exchange_with_peer(&mut peer, TEST_CLOCK_HZ);
        assert_eq!(peer.read_rx_data(), 0x5A);
    }
}
