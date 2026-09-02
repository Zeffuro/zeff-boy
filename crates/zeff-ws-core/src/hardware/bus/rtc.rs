use super::{
    RTC_ACTIVE, RTC_COMMAND_MASK, RTC_READ_DATETIME_COMMAND, RTC_READY, RTC_READY_DELAY_READS,
    RTC_WRITE_DATETIME_COMMAND,
};
use crate::hardware::constants::CPU_CLOCK_HZ;

mod persistence;
pub(super) use persistence::{EXTENSION_LEN, decode_extension, encode_extension, encode_state};

pub(crate) const DETERMINISTIC_RTC_EPOCH: [u8; 7] = [0x00, 0x01, 0x01, 0x06, 0, 0, 0];

#[derive(Clone, Debug)]
pub(super) struct Rtc {
    command: u8,
    payload: [u8; 7],
    payload_index: usize,
    payload_len: usize,
    ready_delay_reads: u8,
    invalid_command: bool,
    subsecond_cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RtcSaveState {
    pub command: u8,
    pub payload: [u8; 7],
    pub payload_index: u8,
    pub payload_len: u8,
    pub ready_delay_reads: u8,
    pub invalid_command: bool,
    pub subsecond_cycles: u32,
}

impl Rtc {
    pub(super) fn new() -> Self {
        Self {
            command: 0,
            payload: DETERMINISTIC_RTC_EPOCH,
            payload_index: 0,
            payload_len: 0,
            ready_delay_reads: 0,
            invalid_command: false,
            subsecond_cycles: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.command = 0;
        self.payload = DETERMINISTIC_RTC_EPOCH;
        self.payload_index = 0;
        self.payload_len = 0;
        self.ready_delay_reads = 0;
        self.invalid_command = false;
        self.subsecond_cycles = 0;
    }

    pub(super) fn step_cycles(&mut self, cycles: u32) {
        let total = u64::from(self.subsecond_cycles) + u64::from(cycles);
        let elapsed_seconds = total / u64::from(CPU_CLOCK_HZ);
        self.subsecond_cycles = (total % u64::from(CPU_CLOCK_HZ)) as u32;
        for _ in 0..elapsed_seconds {
            increment_datetime(&mut self.payload);
        }
    }

    pub(super) fn write_command(&mut self, value: u8, initial_payload: u8) {
        self.command = value & RTC_COMMAND_MASK;
        self.payload_len = rtc_command_length(self.command);
        self.invalid_command = !matches!(self.command, 0x10..=0x1B);
        if self.command == RTC_WRITE_DATETIME_COMMAND && self.payload_len > 0 {
            self.payload[0] = initial_payload;
        }
        self.payload_index = if self.is_write_command() {
            self.payload_len.min(1)
        } else {
            0
        };
        self.ready_delay_reads = RTC_READY_DELAY_READS;
    }

    pub(super) fn write_payload(&mut self, value: u8) {
        if !self.is_write_command() || self.payload_index >= self.payload_len {
            return;
        }
        if self.command == RTC_WRITE_DATETIME_COMMAND {
            self.payload[self.payload_index] = value;
        }
        self.payload_index += 1;
    }

    pub(super) fn read_status(&mut self) -> u8 {
        let status = self.peek_status();
        if self.ready_delay_reads > 0 {
            self.ready_delay_reads -= 1;
        }
        status
    }

    pub(super) fn peek_status(&self) -> u8 {
        let command = self.command & 0x0F;
        if self.invalid_command {
            return RTC_ACTIVE | command;
        }
        if self.ready_delay_reads > 0 {
            return RTC_ACTIVE | command;
        }

        let remaining = self.payload_len.saturating_sub(self.payload_index);
        if self.is_write_command() {
            RTC_READY | (u8::from(remaining > 0) * RTC_ACTIVE) | command
        } else if remaining == 0 {
            command
        } else {
            RTC_READY | (u8::from(remaining > 1) * RTC_ACTIVE) | command
        }
    }

    pub(super) fn read_payload(&mut self) -> u8 {
        if !self.is_read_command() || self.payload_index >= self.payload_len {
            return RTC_READY;
        }

        let value = if self.command == RTC_READ_DATETIME_COMMAND {
            self.payload[self.payload_index]
        } else {
            0
        };
        self.payload_index += 1;
        value
    }

    pub(super) fn peek_payload(&self) -> u8 {
        if self.command == RTC_READ_DATETIME_COMMAND && self.payload_index < self.payload_len {
            self.payload[self.payload_index]
        } else {
            RTC_READY
        }
    }

    pub(crate) fn save_state(&self) -> RtcSaveState {
        RtcSaveState {
            command: self.command,
            payload: self.payload,
            payload_index: self.payload_index as u8,
            payload_len: self.payload_len as u8,
            ready_delay_reads: self.ready_delay_reads,
            invalid_command: self.invalid_command,
            subsecond_cycles: self.subsecond_cycles,
        }
    }

    pub(crate) fn load_state(&mut self, state: RtcSaveState) -> anyhow::Result<()> {
        let payload_len = usize::from(state.payload_len);
        let payload_index = usize::from(state.payload_index);
        let invalid_command = state.command != 0 && !matches!(state.command, 0x10..=0x1B);
        if state.command & !RTC_COMMAND_MASK != 0
            || payload_len != rtc_command_length(state.command)
            || payload_index > payload_len
            || state.ready_delay_reads > RTC_READY_DELAY_READS
            || state.invalid_command != invalid_command
            || state.subsecond_cycles >= CPU_CLOCK_HZ
        {
            anyhow::bail!("invalid WonderSwan RTC state");
        }
        self.command = state.command;
        self.payload = state.payload;
        self.payload_index = payload_index;
        self.payload_len = payload_len;
        self.ready_delay_reads = state.ready_delay_reads;
        self.invalid_command = state.invalid_command;
        self.subsecond_cycles = state.subsecond_cycles;
        Ok(())
    }

    fn is_read_command(&self) -> bool {
        self.command & 1 != 0 && !self.invalid_command
    }

    fn is_write_command(&self) -> bool {
        self.command & 1 == 0 && !self.invalid_command
    }
}

fn rtc_command_length(command: u8) -> usize {
    match command {
        0x10..=0x13 => 1,
        0x14..=0x15 => 7,
        0x16..=0x17 => 3,
        0x18..=0x1B => 2,
        _ => 0,
    }
}

fn increment_datetime(payload: &mut [u8; 7]) {
    let mut second = bcd_to_u8(payload[6]) + 1;
    if second < 60 {
        payload[6] = u8_to_bcd(second);
        return;
    }
    second = 0;
    let mut minute = bcd_to_u8(payload[5]) + 1;
    if minute < 60 {
        payload[5] = u8_to_bcd(minute);
        payload[6] = u8_to_bcd(second);
        return;
    }
    minute = 0;
    let mut hour = bcd_to_u8(payload[4]) + 1;
    if hour < 24 {
        payload[4] = u8_to_bcd(hour);
        payload[5] = u8_to_bcd(minute);
        payload[6] = u8_to_bcd(second);
        return;
    }
    hour = 0;
    let year = bcd_to_u8(payload[0]);
    let month = bcd_to_u8(payload[1]);
    let mut day = bcd_to_u8(payload[2]) + 1;
    if day > days_in_month(year, month) {
        day = 1;
        let next_month = month + 1;
        if next_month > 12 {
            payload[0] = u8_to_bcd((year + 1) % 100);
            payload[1] = 0x01;
        } else {
            payload[1] = u8_to_bcd(next_month);
        }
    }
    payload[2] = u8_to_bcd(day);
    payload[3] = (payload[3] + 1) % 7;
    payload[4] = u8_to_bcd(hour);
    payload[5] = u8_to_bcd(minute);
    payload[6] = u8_to_bcd(second);
}

pub(crate) fn valid_datetime(payload: [u8; 7]) -> bool {
    let year = bcd_to_u8(payload[0]);
    let month = bcd_to_u8(payload[1]);
    let day = bcd_to_u8(payload[2]);
    valid_bcd(payload[0])
        && valid_bcd(payload[1])
        && valid_bcd(payload[2])
        && valid_bcd(payload[4])
        && valid_bcd(payload[5])
        && valid_bcd(payload[6])
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && payload[3] < 7
        && bcd_to_u8(payload[4]) < 24
        && bcd_to_u8(payload[5]) < 60
        && bcd_to_u8(payload[6]) < 60
}

fn days_in_month(year: u8, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) => 29,
        2 => 28,
        _ => 0,
    }
}

fn valid_bcd(value: u8) -> bool {
    value & 0x0F < 10 && value >> 4 < 10
}

fn bcd_to_u8(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 0x0F)
}

fn u8_to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}
