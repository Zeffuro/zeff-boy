use super::Mbc3;
use crate::hardware::cartridge::rtc::{RTC_REG_COUNT, now_unix_seconds, sanitize_rtc_register};

impl Mbc3 {
    pub(in crate::hardware::cartridge) fn save_len(&self) -> usize {
        if self.has_rtc {
            self.ram.len() + 48
        } else {
            self.ram.len()
        }
    }

    pub(in crate::hardware::cartridge) fn dump_sram(&self) -> Vec<u8> {
        if !self.has_rtc {
            return self.ram.clone();
        }

        let now = now_unix_seconds();
        let rtc = &self.rtc;

        let mut bytes = Vec::with_capacity(self.ram.len() + 48);
        bytes.extend_from_slice(&self.ram);

        for value in rtc.internal {
            bytes.extend_from_slice(&(value as u32).to_le_bytes());
        }
        for value in rtc.latched {
            bytes.extend_from_slice(&(value as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&now.to_le_bytes());

        bytes
    }

    pub(in crate::hardware::cartridge) fn load_sram(&mut self, bytes: &[u8]) {
        if !self.has_rtc {
            self.load_ram_bytes(bytes);
            return;
        }

        let ram_len = self.ram.len();
        if bytes.len() == ram_len + 44 || bytes.len() == ram_len + 48 {
            self.load_ram_bytes(&bytes[..ram_len]);
            let rtc = &mut self.rtc;

            let footer = &bytes[ram_len..ram_len + 44];
            for i in 0..RTC_REG_COUNT {
                let start = i * 4;
                let mut reg = [0u8; 4];
                reg.copy_from_slice(&footer[start..start + 4]);
                rtc.internal[i] = sanitize_rtc_register(i, u32::from_le_bytes(reg) as u8);
            }
            for i in 0..RTC_REG_COUNT {
                let start = (RTC_REG_COUNT + i) * 4;
                let mut reg = [0u8; 4];
                reg.copy_from_slice(&footer[start..start + 4]);
                rtc.latched[i] = sanitize_rtc_register(i, u32::from_le_bytes(reg) as u8);
            }

            // Read the saved wall-clock timestamp (in seconds) and catch up.
            let saved_seconds = if bytes.len() == ram_len + 48 {
                let mut ts = [0u8; 8];
                ts.copy_from_slice(&bytes[ram_len + 40..ram_len + 48]);
                u64::from_le_bytes(ts)
            } else {
                let mut ts = [0u8; 4];
                ts.copy_from_slice(&bytes[ram_len + 40..ram_len + 44]);
                u32::from_le_bytes(ts) as u64
            };

            let now = now_unix_seconds();
            let elapsed = now.saturating_sub(saved_seconds);
            rtc.catchup_seconds(elapsed);
            rtc.subsecond_cycles = 0;
            return;
        }

        self.load_ram_bytes(bytes);
    }
}
