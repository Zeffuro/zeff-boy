use super::{
    RTC_ACTIVE, RTC_COMMAND_MASK, RTC_READ_DATETIME_COMMAND, RTC_READY, RTC_READY_DELAY_READS,
    RTC_WRITE_DATETIME_COMMAND,
};

#[derive(Clone, Debug)]
pub(super) struct Rtc {
    command: u8,
    payload: [u8; 7],
    payload_index: usize,
    payload_len: usize,
    ready_delay_reads: u8,
    invalid_command: bool,
}

impl Rtc {
    pub(super) fn new() -> Self {
        Self {
            command: 0,
            payload: default_rtc_payload(),
            payload_index: 0,
            payload_len: 0,
            ready_delay_reads: 0,
            invalid_command: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.command = 0;
        self.payload = default_rtc_payload();
        self.payload_index = 0;
        self.payload_len = 0;
        self.ready_delay_reads = 0;
        self.invalid_command = false;
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

fn default_rtc_payload() -> [u8; 7] {
    [
        0x00, // year
        0x01, // month
        0x01, // day of month
        0x00, // day of week
        0x00, // hour
        0x00, // minute
        0x00, // second
    ]
}
