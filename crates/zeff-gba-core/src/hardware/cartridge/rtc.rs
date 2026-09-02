use anyhow::{Result, bail};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::hardware::constants::CPU_CLOCK_HZ;

const GPIO_DATA: u32 = 0x0800_00C4;
const GPIO_DIRECTION: u32 = 0x0800_00C6;
const GPIO_CONTROL: u32 = 0x0800_00C8;
const GPIO_MASK: u8 = 0x0F;
const SCK: u8 = 0x01;
const SIO: u8 = 0x02;
const CS: u8 = 0x04;
const CONTROL_HOUR_24: u8 = 0x40;

pub(super) mod persistence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtcDateTime {
    year: u8,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtcState {
    pub data_latch: u8,
    pub pin_state: u8,
    pub direction: u8,
    pub read_write: bool,
    pub transfer_step: u8,
    pub last_sck: bool,
    pub read_bit_sampled: bool,
    pub bits_read: u8,
    pub bits: u8,
    pub command: Option<u8>,
    pub command_reading: bool,
    pub bytes_remaining: u8,
    pub transfer_bytes: [u8; 7],
    pub transfer_index: u8,
    pub control: u8,
    pub date_time: RtcDateTime,
    pub subsecond_cycles: u32,
}

impl RtcDateTime {
    pub fn new(year: u16, month: u8, day: u8, weekday: u8, time: [u8; 3]) -> Result<Self> {
        let [hour, minute, second] = time;
        if !(2000..=2099).contains(&year)
            || !(1..=12).contains(&month)
            || !(1..=days_in_month((year - 2000) as u8, month)).contains(&day)
            || weekday > 6
            || hour > 23
            || minute > 59
            || second > 59
        {
            bail!("invalid GBA RTC date/time");
        }
        Ok(Self {
            year: (year - 2000) as u8,
            month,
            day,
            weekday,
            hour,
            minute,
            second,
        })
    }

    pub fn year(self) -> u16 {
        u16::from(self.year) + 2000
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    pub fn weekday(self) -> u8 {
        self.weekday
    }

    pub fn hour(self) -> u8 {
        self.hour
    }

    pub fn minute(self) -> u8 {
        self.minute
    }

    pub fn second(self) -> u8 {
        self.second
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Reset,
    DateTime,
    ForceIrq,
    Control,
    Time,
}

impl Command {
    fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(Self::Reset),
            2 => Some(Self::DateTime),
            3 => Some(Self::ForceIrq),
            4 => Some(Self::Control),
            6 => Some(Self::Time),
            _ => None,
        }
    }

    fn bits(self) -> u8 {
        match self {
            Self::Reset => 0,
            Self::DateTime => 2,
            Self::ForceIrq => 3,
            Self::Control => 4,
            Self::Time => 6,
        }
    }

    fn byte_len(self) -> u8 {
        match self {
            Self::DateTime => 7,
            Self::Control => 1,
            Self::Time => 3,
            Self::Reset | Self::ForceIrq => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Calendar {
    year: u8,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            year: 0,
            month: 1,
            day: 1,
            weekday: 6,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }
}

impl Calendar {
    fn step_second(&mut self) {
        self.second += 1;
        if self.second < 60 {
            return;
        }
        self.second = 0;
        self.minute += 1;
        if self.minute < 60 {
            return;
        }
        self.minute = 0;
        self.hour += 1;
        if self.hour < 24 {
            return;
        }
        self.hour = 0;
        self.weekday = (self.weekday + 1) % 7;
        self.day += 1;
        if self.day <= days_in_month(self.year, self.month) {
            return;
        }
        self.day = 1;
        self.month += 1;
        if self.month <= 12 {
            return;
        }
        self.month = 1;
        self.year = (self.year + 1) % 100;
    }

    fn date_time_bytes(self, control: u8) -> [u8; 7] {
        let mut hour = to_bcd(self.hour);
        if control & CONTROL_HOUR_24 == 0 {
            let pm = self.hour >= 12;
            let hour_12 = match self.hour % 12 {
                0 => 12,
                value => value,
            };
            hour = to_bcd(hour_12) | if pm { 0x80 } else { 0 };
        }
        [
            to_bcd(self.year),
            to_bcd(self.month),
            to_bcd(self.day),
            to_bcd(self.weekday),
            hour,
            to_bcd(self.minute),
            to_bcd(self.second),
        ]
    }

    fn set_date_time(&mut self, bytes: &[u8], control: u8) {
        if bytes.len() != 7 {
            return;
        }
        let Some(year) = from_bcd(bytes[0]) else {
            return;
        };
        let Some(month) = from_bcd(bytes[1]) else {
            return;
        };
        let Some(day) = from_bcd(bytes[2]) else {
            return;
        };
        let Some(weekday) = from_bcd(bytes[3]) else {
            return;
        };
        let Some((hour, minute, second)) = decode_time(&bytes[4..], control) else {
            return;
        };
        if !(1..=12).contains(&month)
            || !(1..=days_in_month(year, month)).contains(&day)
            || weekday > 6
            || hour > 23
            || minute > 59
            || second > 59
        {
            return;
        }
        self.year = year;
        self.month = month;
        self.day = day;
        self.weekday = weekday;
        self.hour = hour;
        self.minute = minute;
        self.second = second;
    }

    fn set_time(&mut self, bytes: &[u8], control: u8) {
        let Some((hour, minute, second)) = decode_time(bytes, control) else {
            return;
        };
        self.hour = hour;
        self.minute = minute;
        self.second = second;
    }
}

#[derive(Clone, Debug)]
pub(super) struct RtcGpio {
    data_latch: u8,
    pin_state: u8,
    direction: u8,
    read_write: bool,
    transfer_step: u8,
    last_sck: bool,
    read_bit_sampled: bool,
    bits_read: u8,
    bits: u8,
    command: Option<Command>,
    command_reading: bool,
    bytes_remaining: u8,
    transfer_bytes: [u8; 7],
    transfer_index: u8,
    control: u8,
    calendar: Calendar,
    subsecond_cycles: u32,
}

impl Default for RtcGpio {
    fn default() -> Self {
        Self {
            data_latch: 0,
            pin_state: 0,
            direction: 0,
            read_write: false,
            transfer_step: 0,
            last_sck: false,
            read_bit_sampled: false,
            bits_read: 0,
            bits: 0,
            command: None,
            command_reading: false,
            bytes_remaining: 0,
            transfer_bytes: [0; 7],
            transfer_index: 0,
            control: CONTROL_HOUR_24,
            calendar: Calendar::default(),
            subsecond_cycles: 0,
        }
    }
}

impl RtcGpio {
    pub(super) fn state(&self) -> RtcState {
        RtcState {
            data_latch: self.data_latch,
            pin_state: self.pin_state,
            direction: self.direction,
            read_write: self.read_write,
            transfer_step: self.transfer_step,
            last_sck: self.last_sck,
            read_bit_sampled: self.read_bit_sampled,
            bits_read: self.bits_read,
            bits: self.bits,
            command: self.command.map(Command::bits),
            command_reading: self.command_reading,
            bytes_remaining: self.bytes_remaining,
            transfer_bytes: self.transfer_bytes,
            transfer_index: self.transfer_index,
            control: self.control,
            date_time: self.date_time(),
            subsecond_cycles: self.subsecond_cycles,
        }
    }

    pub(super) fn date_time(&self) -> RtcDateTime {
        RtcDateTime {
            year: self.calendar.year,
            month: self.calendar.month,
            day: self.calendar.day,
            weekday: self.calendar.weekday,
            hour: self.calendar.hour,
            minute: self.calendar.minute,
            second: self.calendar.second,
        }
    }

    pub(super) fn set_date_time(&mut self, date_time: RtcDateTime) {
        self.calendar = Calendar {
            year: date_time.year,
            month: date_time.month,
            day: date_time.day,
            weekday: date_time.weekday,
            hour: date_time.hour,
            minute: date_time.minute,
            second: date_time.second,
        };
        self.subsecond_cycles = 0;
    }

    pub(super) fn read8(&self, addr: u32) -> Option<u8> {
        if !self.read_write || !(GPIO_DATA..=GPIO_CONTROL + 1).contains(&addr) {
            return None;
        }
        let value = match addr & !1 {
            GPIO_DATA => self.pin_state,
            GPIO_DIRECTION => self.direction,
            GPIO_CONTROL => u8::from(self.read_write),
            _ => return None,
        };
        Some(if addr & 1 == 0 { value } else { 0 })
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> bool {
        match addr {
            GPIO_DATA => {
                self.data_latch = value as u8 & GPIO_MASK;
                self.pin_state =
                    (self.pin_state & !self.direction) | (self.data_latch & self.direction);
                self.observe_pins();
                true
            }
            GPIO_DIRECTION => {
                self.direction = value as u8 & GPIO_MASK;
                self.pin_state =
                    (self.pin_state & !self.direction) | (self.data_latch & self.direction);
                self.observe_pins();
                if self.transfer_step == 2 && self.command_reading {
                    self.drive_sio(self.output_bit());
                }
                true
            }
            GPIO_CONTROL => {
                self.read_write = value & 1 != 0;
                true
            }
            _ => false,
        }
    }

    pub(super) fn step_cycles(&mut self, cycles: u32) {
        self.subsecond_cycles = self.subsecond_cycles.saturating_add(cycles);
        while self.subsecond_cycles >= CPU_CLOCK_HZ {
            self.subsecond_cycles -= CPU_CLOCK_HZ;
            self.calendar.step_second();
        }
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.data_latch);
        writer.write_u8(self.pin_state);
        writer.write_u8(self.direction);
        writer.write_bool(self.read_write);
        writer.write_u8(self.transfer_step);
        writer.write_bool(self.last_sck);
        writer.write_bool(self.read_bit_sampled);
        writer.write_u8(self.bits_read);
        writer.write_u8(self.bits);
        writer.write_u8(self.command.map_or(0xFF, Command::bits));
        writer.write_bool(self.command_reading);
        writer.write_u8(self.bytes_remaining);
        writer.write_bytes(&self.transfer_bytes);
        writer.write_u8(self.transfer_index);
        writer.write_u8(self.control);
        writer.write_u8(self.calendar.year);
        writer.write_u8(self.calendar.month);
        writer.write_u8(self.calendar.day);
        writer.write_u8(self.calendar.weekday);
        writer.write_u8(self.calendar.hour);
        writer.write_u8(self.calendar.minute);
        writer.write_u8(self.calendar.second);
        writer.write_u32(self.subsecond_cycles);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let data_latch = reader.read_u8()? & GPIO_MASK;
        let pin_state = reader.read_u8()? & GPIO_MASK;
        let direction = reader.read_u8()? & GPIO_MASK;
        let read_write = reader.read_bool()?;
        let transfer_step = reader.read_u8()?;
        let last_sck = reader.read_bool()?;
        let read_bit_sampled = reader.read_bool()?;
        let bits_read = reader.read_u8()?;
        let bits = reader.read_u8()?;
        let command = match reader.read_u8()? {
            0xFF => None,
            bits => Some(
                Command::from_bits(bits)
                    .ok_or_else(|| anyhow::anyhow!("invalid GBA RTC command"))?,
            ),
        };
        let command_reading = reader.read_bool()?;
        let bytes_remaining = reader.read_u8()?;
        let mut transfer_bytes = [0; 7];
        reader.read_exact(&mut transfer_bytes)?;
        let transfer_index = reader.read_u8()?;
        let control = reader.read_u8()? & 0x6A;
        let calendar = Calendar {
            year: reader.read_u8()?,
            month: reader.read_u8()?,
            day: reader.read_u8()?,
            weekday: reader.read_u8()?,
            hour: reader.read_u8()?,
            minute: reader.read_u8()?,
            second: reader.read_u8()?,
        };
        let subsecond_cycles = reader.read_u32()?;
        if transfer_step > 2
            || bits_read > 7
            || bytes_remaining > 7
            || transfer_index > 7
            || calendar.year > 99
            || !(1..=12).contains(&calendar.month)
            || !(1..=days_in_month(calendar.year, calendar.month)).contains(&calendar.day)
            || calendar.weekday > 6
            || calendar.hour > 23
            || calendar.minute > 59
            || calendar.second > 59
            || subsecond_cycles >= CPU_CLOCK_HZ
        {
            bail!("invalid GBA RTC state");
        }
        Ok(Self {
            data_latch,
            pin_state,
            direction,
            read_write,
            transfer_step,
            last_sck,
            read_bit_sampled,
            bits_read,
            bits,
            command,
            command_reading,
            bytes_remaining,
            transfer_bytes,
            transfer_index,
            control,
            calendar,
            subsecond_cycles,
        })
    }

    fn observe_pins(&mut self) {
        match self.transfer_step {
            0 => {
                if self.pin_state & (SCK | CS) == SCK {
                    self.transfer_step = 1;
                } else if self.pin_state & (SCK | CS) == SCK | CS {
                    self.transfer_step = 2;
                    self.last_sck = true;
                }
            }
            1 => {
                if self.pin_state & (SCK | CS) == SCK | CS {
                    self.transfer_step = 2;
                    self.last_sck = true;
                } else if self.pin_state & (SCK | CS) != SCK {
                    self.transfer_step = 0;
                }
            }
            2 => {
                let sck = self.pin_state & SCK != 0;
                if self.pin_state & CS == 0 {
                    self.abort_transfer();
                    self.transfer_step = u8::from(sck);
                } else if !self.command_reading && !self.last_sck && sck {
                    self.bits |= ((self.pin_state & SIO) >> 1) << self.bits_read;
                    self.bits_read += 1;
                    if self.bits_read == 8 {
                        self.process_input_byte();
                    }
                    self.last_sck = sck;
                } else if self.command_reading && self.last_sck && !sck {
                    if self.read_bit_sampled {
                        self.bits_read += 1;
                        if self.bits_read == 8 {
                            self.finish_output_byte();
                        } else {
                            self.drive_sio(self.output_bit());
                        }
                        self.read_bit_sampled = false;
                    }
                    self.last_sck = sck;
                } else {
                    if self.command_reading && !self.last_sck && sck {
                        self.read_bit_sampled = true;
                    }
                    self.last_sck = sck;
                }
            }
            _ => unreachable!(),
        }
    }

    fn process_input_byte(&mut self) {
        let byte = self.bits;
        self.bits = 0;
        self.bits_read = 0;
        if let Some(command) = self.command {
            self.store_command_byte(command, byte);
            return;
        }
        if byte & 0x0F != 0x06 {
            self.abort_transfer();
            return;
        }
        let Some(command) = Command::from_bits((byte >> 4) & 0x07) else {
            self.abort_transfer();
            return;
        };
        self.command = Some(command);
        self.command_reading = byte & 0x80 != 0;
        self.bytes_remaining = command.byte_len();
        self.transfer_index = 0;
        if self.command_reading {
            self.prepare_output(command);
        }
        if self.bytes_remaining == 0 {
            if command == Command::Reset {
                self.control = 0;
            }
            self.abort_transfer();
        }
    }

    fn store_command_byte(&mut self, command: Command, byte: u8) {
        let index = usize::from(self.transfer_index);
        if index < self.transfer_bytes.len() {
            self.transfer_bytes[index] = byte;
        }
        self.transfer_index = self.transfer_index.saturating_add(1);
        self.bytes_remaining = self.bytes_remaining.saturating_sub(1);
        if self.bytes_remaining != 0 {
            return;
        }
        match command {
            Command::Control => self.control = self.transfer_bytes[0] & 0x6A,
            Command::DateTime => self
                .calendar
                .set_date_time(&self.transfer_bytes[..7], self.control),
            Command::Time => self
                .calendar
                .set_time(&self.transfer_bytes[..3], self.control),
            Command::Reset => {
                self.control = 0;
            }
            Command::ForceIrq => {}
        }
        self.abort_transfer();
    }

    fn prepare_output(&mut self, command: Command) {
        self.transfer_bytes = match command {
            Command::Control => [self.control, 0, 0, 0, 0, 0, 0],
            Command::DateTime => self.calendar.date_time_bytes(self.control),
            Command::Time => {
                let date_time = self.calendar.date_time_bytes(self.control);
                [date_time[4], date_time[5], date_time[6], 0, 0, 0, 0]
            }
            Command::Reset | Command::ForceIrq => [0; 7],
        };
    }

    fn output_bit(&self) -> u8 {
        let byte = self.transfer_bytes[usize::from(self.transfer_index)];
        (byte >> self.bits_read) & 1
    }

    fn finish_output_byte(&mut self) {
        self.bits_read = 0;
        self.transfer_index = self.transfer_index.saturating_add(1);
        self.bytes_remaining = self.bytes_remaining.saturating_sub(1);
        if self.bytes_remaining == 0 {
            self.abort_transfer();
        } else {
            self.drive_sio(self.output_bit());
        }
    }

    fn drive_sio(&mut self, bit: u8) {
        if self.direction & SIO == 0 {
            self.pin_state = (self.pin_state & !SIO) | ((bit & 1) << 1);
        }
    }

    fn abort_transfer(&mut self) {
        self.bits_read = 0;
        self.bits = 0;
        self.command = None;
        self.command_reading = false;
        self.bytes_remaining = 0;
        self.transfer_index = 0;
        self.read_bit_sampled = false;
    }
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

fn to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

fn from_bcd(value: u8) -> Option<u8> {
    let high = value >> 4;
    let low = value & 0x0F;
    (high <= 9 && low <= 9).then_some(high * 10 + low)
}

fn decode_hour(value: u8, control: u8) -> Option<u8> {
    let hour = from_bcd(value & 0x3F)?;
    if control & CONTROL_HOUR_24 != 0 {
        return (hour < 24).then_some(hour);
    }
    if !(1..=12).contains(&hour) {
        return None;
    }
    let pm = value & 0x80 != 0;
    Some(match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (value, true) => value + 12,
        (value, false) => value,
    })
}

fn decode_time(bytes: &[u8], control: u8) -> Option<(u8, u8, u8)> {
    let [hour, minute, second] = bytes else {
        return None;
    };
    let hour = decode_hour(*hour, control)?;
    let minute = from_bcd(*minute)?;
    let second = from_bcd(*second)?;
    (hour <= 23 && minute <= 59 && second <= 59).then_some((hour, minute, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_public_date_time() {
        let date_time = RtcDateTime::new(2024, 2, 29, 4, [23, 59, 59]).unwrap();
        assert_eq!(date_time.year(), 2024);
        assert_eq!(date_time.month(), 2);
        assert_eq!(date_time.day(), 29);
        assert_eq!(date_time.weekday(), 4);
        assert_eq!(date_time.hour(), 23);
        assert_eq!(date_time.minute(), 59);
        assert_eq!(date_time.second(), 59);

        assert!(RtcDateTime::new(2023, 2, 29, 3, [0, 0, 0]).is_err());
        assert!(RtcDateTime::new(2100, 1, 1, 5, [0, 0, 0]).is_err());
    }

    fn arm(rtc: &mut RtcGpio) {
        rtc.write16(GPIO_DATA, 1);
        rtc.write16(GPIO_DATA, 5);
        rtc.write16(GPIO_DIRECTION, 7);
        assert_eq!(rtc.transfer_step, 2);
    }

    fn write_bit(rtc: &mut RtcGpio, bit: u8) {
        rtc.write16(GPIO_DATA, u16::from(CS | ((bit & 1) << 1)));
        rtc.write16(GPIO_DATA, u16::from(CS | SCK | ((bit & 1) << 1)));
    }

    fn write_command(rtc: &mut RtcGpio, command: u8, read: bool) {
        let byte = 0x60 | (command << 1) | u8::from(read);
        for bit in (0..8).rev() {
            write_bit(rtc, (byte >> bit) & 1);
        }
    }

    fn write_data(rtc: &mut RtcGpio, byte: u8) {
        for bit in 0..8 {
            write_bit(rtc, (byte >> bit) & 1);
        }
    }

    fn read_data(rtc: &mut RtcGpio, count: usize) -> Vec<u8> {
        rtc.write16(GPIO_DIRECTION, u16::from(SCK | CS));
        let mut bytes = Vec::with_capacity(count);
        for _ in 0..count {
            let mut byte = 0;
            for bit in 0..8 {
                rtc.write16(GPIO_DATA, u16::from(CS));
                rtc.write16(GPIO_DATA, u16::from(CS | SCK));
                byte |= (rtc.read8(GPIO_DATA).unwrap() >> 1 & 1) << bit;
            }
            bytes.push(byte);
        }
        bytes
    }

    #[test]
    fn emerald_latch_order_arms_gpio_and_reads_control_lsb_first() {
        let mut rtc = RtcGpio::default();
        arm(&mut rtc);
        rtc.write16(GPIO_CONTROL, 1);
        write_command(&mut rtc, 1, true);

        assert_eq!(read_data(&mut rtc, 1), vec![CONTROL_HOUR_24]);
    }

    #[test]
    fn datetime_and_time_commands_return_bcd_data() {
        let mut rtc = RtcGpio {
            calendar: Calendar {
                year: 24,
                month: 2,
                day: 29,
                weekday: 4,
                hour: 13,
                minute: 5,
                second: 9,
            },
            ..RtcGpio::default()
        };
        arm(&mut rtc);
        rtc.write16(GPIO_CONTROL, 1);
        write_command(&mut rtc, 2, true);
        assert_eq!(
            read_data(&mut rtc, 7),
            vec![0x24, 0x02, 0x29, 0x04, 0x13, 0x05, 0x09]
        );

        arm(&mut rtc);
        write_command(&mut rtc, 3, true);
        assert_eq!(read_data(&mut rtc, 3), vec![0x13, 0x05, 0x09]);
    }

    #[test]
    fn reset_clears_control_without_rewinding_calendar() {
        let mut rtc = RtcGpio {
            control: CONTROL_HOUR_24,
            calendar: Calendar {
                second: 41,
                ..Calendar::default()
            },
            ..RtcGpio::default()
        };
        arm(&mut rtc);
        write_command(&mut rtc, 0, false);

        assert_eq!(rtc.control, 0);
        assert_eq!(rtc.calendar.second, 41);
    }

    #[test]
    fn cycle_clock_rolls_through_leap_day() {
        let mut rtc = RtcGpio {
            calendar: Calendar {
                year: 0,
                month: 2,
                day: 28,
                weekday: 1,
                hour: 23,
                minute: 59,
                second: 59,
            },
            ..RtcGpio::default()
        };
        rtc.step_cycles(CPU_CLOCK_HZ);

        assert_eq!(rtc.calendar.month, 2);
        assert_eq!(rtc.calendar.day, 29);
        assert_eq!(rtc.calendar.weekday, 2);
        assert_eq!(rtc.calendar.hour, 0);
        assert_eq!(rtc.calendar.minute, 0);
        assert_eq!(rtc.calendar.second, 0);
    }

    #[test]
    fn state_roundtrip_continues_partial_datetime_write() {
        let mut rtc = RtcGpio::default();
        arm(&mut rtc);
        write_command(&mut rtc, 2, false);
        for bit in 0..3 {
            write_bit(&mut rtc, (0x24 >> bit) & 1);
        }
        let mut writer = StateWriter::new();
        rtc.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut restored = RtcGpio::read_state(&mut reader).unwrap();

        for bit in 3..8 {
            write_bit(&mut restored, (0x24 >> bit) & 1);
        }
        for byte in [0x02, 0x29, 0x04, 0x13, 0x05, 0x09] {
            write_data(&mut restored, byte);
        }

        assert_eq!(
            restored.calendar.date_time_bytes(restored.control)[..],
            [0x24, 0x02, 0x29, 0x04, 0x13, 0x05, 0x09]
        );
    }
}

#[cfg(test)]
mod tas_tests;
