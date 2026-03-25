use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const RTC_SECONDS: usize = 0;
pub(super) const RTC_MINUTES: usize = 1;
pub(super) const RTC_HOURS: usize = 2;
pub(super) const RTC_DAY_LOW: usize = 3;
pub(super) const RTC_DAY_HIGH: usize = 4;
pub(super) const RTC_REG_COUNT: usize = 5;

pub(super) const RTC_DH_DAY_HIGH_BIT: u8 = 0x01;
pub(super) const RTC_DH_HALT_BIT: u8 = 0x40;
pub(super) const RTC_DH_CARRY_BIT: u8 = 0x80;

pub(super) const T_CYCLES_PER_SECOND: u64 = 4_194_304;

#[derive(Clone)]
pub(super) struct Rtc {
    pub(super) internal: [u8; RTC_REG_COUNT],
    pub(super) latched: [u8; RTC_REG_COUNT],
    pub(super) subsecond_cycles: u64,
}

impl Rtc {
    pub(super) fn new() -> Self {
        Self {
            internal: [0; RTC_REG_COUNT],
            latched: [0; RTC_REG_COUNT],
            subsecond_cycles: 0,
        }
    }

    pub(super) fn advance_cycles(&mut self, t_cycles: u64) {
        if self.internal[RTC_DAY_HIGH] & RTC_DH_HALT_BIT != 0 {
            return;
        }

        self.subsecond_cycles = self.subsecond_cycles.saturating_add(t_cycles);

        while self.subsecond_cycles >= T_CYCLES_PER_SECOND {
            self.subsecond_cycles -= T_CYCLES_PER_SECOND;
            self.tick_one_second();
        }
    }

    pub(super) fn catchup_seconds(&mut self, seconds: u64) {
        if self.internal[RTC_DAY_HIGH] & RTC_DH_HALT_BIT != 0 {
            return;
        }

        for _ in 0..seconds {
            self.tick_one_second();
        }
    }

    pub(super) fn latch(&mut self) {
        self.latched = self.internal;
    }

    pub(super) fn read_latched(&self, rtc_select: u8) -> u8 {
        self.latched[rtc_index(rtc_select)]
    }

    pub(super) fn write_internal(&mut self, rtc_select: u8, value: u8) {
        let idx = rtc_index(rtc_select);
        self.internal[idx] = sanitize_rtc_register(idx, value);

        if idx == RTC_SECONDS {
            self.subsecond_cycles = 0;
        }
    }

    fn tick_one_second(&mut self) {
        let seconds = self.internal[RTC_SECONDS];
        if seconds == 59 {
            self.internal[RTC_SECONDS] = 0;
            self.increment_minutes();
        } else {
            self.internal[RTC_SECONDS] = seconds.wrapping_add(1) & 0x3F;
        }
    }

    fn increment_minutes(&mut self) {
        let minutes = self.internal[RTC_MINUTES];
        if minutes == 59 {
            self.internal[RTC_MINUTES] = 0;
            self.increment_hours();
        } else {
            self.internal[RTC_MINUTES] = minutes.wrapping_add(1) & 0x3F;
        }
    }

    fn increment_hours(&mut self) {
        let hours = self.internal[RTC_HOURS];
        if hours == 23 {
            self.internal[RTC_HOURS] = 0;
            self.increment_days();
        } else {
            self.internal[RTC_HOURS] = hours.wrapping_add(1) & 0x1F;
        }
    }

    fn increment_days(&mut self) {
        let day = ((self.internal[RTC_DAY_HIGH] & RTC_DH_DAY_HIGH_BIT) as u16) << 8
            | self.internal[RTC_DAY_LOW] as u16;
        let (next_day, overflowed) = if day == 0x1FF {
            (0, true)
        } else {
            (day + 1, false)
        };

        self.internal[RTC_DAY_LOW] = (next_day & 0xFF) as u8;
        let mut dh = self.internal[RTC_DAY_HIGH] & RTC_DH_HALT_BIT;
        dh |= ((next_day >> 8) as u8) & RTC_DH_DAY_HIGH_BIT;
        if (self.internal[RTC_DAY_HIGH] & RTC_DH_CARRY_BIT != 0) || overflowed {
            dh |= RTC_DH_CARRY_BIT;
        }
        self.internal[RTC_DAY_HIGH] = dh;
    }
}

pub(super) fn sanitize_rtc_register(index: usize, value: u8) -> u8 {
    match index {
        RTC_SECONDS => value & 0x3F,
        RTC_MINUTES => value & 0x3F,
        RTC_HOURS => value & 0x1F,
        RTC_DAY_LOW => value,
        RTC_DAY_HIGH => value & (RTC_DH_DAY_HIGH_BIT | RTC_DH_HALT_BIT | RTC_DH_CARRY_BIT),
        _ => 0,
    }
}

pub(super) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rtc_index(rtc_select: u8) -> usize {
    (rtc_select - 0x08) as usize
}
