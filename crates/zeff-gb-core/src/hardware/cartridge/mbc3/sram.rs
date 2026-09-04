use super::Mbc3;
use crate::hardware::cartridge::rtc::{RTC_REG_COUNT, now_unix_seconds, sanitize_rtc_register};
use crate::hardware::types::constants::GB_T_CYCLES_PER_SECOND;

const RTC_SUBSECOND_V1_MAGIC: &[u8; 8] = b"ZBRTC001";
const RTC_SUBSECOND_EXTENSION_LEN: usize = 16;

impl Mbc3 {
    pub(in crate::hardware::cartridge) fn save_len(&self) -> usize {
        if self.has_rtc {
            self.ram.len() + 48
        } else {
            self.ram.len()
        }
    }

    pub(in crate::hardware::cartridge) fn dump_sram(&self) -> Vec<u8> {
        self.dump_sram_at_time(now_unix_seconds())
    }

    pub(in crate::hardware::cartridge) fn dump_sram_at_time(&self, now: u64) -> Vec<u8> {
        if !self.has_rtc {
            return self.ram.clone();
        }

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

    pub(in crate::hardware::cartridge) fn dump_sram_with_rtc_subsecond(&self) -> Vec<u8> {
        self.dump_sram_with_rtc_subsecond_at_time(now_unix_seconds())
    }

    pub(in crate::hardware::cartridge) fn dump_sram_with_rtc_subsecond_at_time(
        &self,
        now: u64,
    ) -> Vec<u8> {
        let mut bytes = self.dump_sram_at_time(now);
        if self.has_rtc {
            bytes.reserve(RTC_SUBSECOND_EXTENSION_LEN);
            bytes.extend_from_slice(RTC_SUBSECOND_V1_MAGIC);
            bytes.extend_from_slice(&self.rtc.subsecond_cycles.to_le_bytes());
        }
        bytes
    }

    pub(in crate::hardware::cartridge) fn load_sram(&mut self, bytes: &[u8]) {
        self.load_sram_at_time(bytes, now_unix_seconds());
    }

    pub(in crate::hardware::cartridge) fn load_sram_at_time(&mut self, bytes: &[u8], now: u64) {
        if !self.has_rtc {
            self.load_ram_bytes(bytes);
            return;
        }

        let ram_len = self.ram.len();
        let extended_subsecond = (bytes.len() == ram_len + 48 + RTC_SUBSECOND_EXTENSION_LEN
            && &bytes[ram_len + 48..ram_len + 56] == RTC_SUBSECOND_V1_MAGIC)
            .then(|| {
                let mut subsecond = [0; 8];
                subsecond.copy_from_slice(&bytes[ram_len + 56..ram_len + 64]);
                u64::from_le_bytes(subsecond)
            })
            .filter(|subsecond| *subsecond < GB_T_CYCLES_PER_SECOND);
        if bytes.len() == ram_len + 44
            || bytes.len() == ram_len + 48
            || extended_subsecond.is_some()
        {
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
            let saved_seconds = if bytes.len() >= ram_len + 48 {
                let mut ts = [0u8; 8];
                ts.copy_from_slice(&bytes[ram_len + 40..ram_len + 48]);
                u64::from_le_bytes(ts)
            } else {
                let mut ts = [0u8; 4];
                ts.copy_from_slice(&bytes[ram_len + 40..ram_len + 44]);
                u32::from_le_bytes(ts) as u64
            };

            let elapsed = now.saturating_sub(saved_seconds);
            rtc.catchup_seconds(elapsed);
            rtc.subsecond_cycles = extended_subsecond.unwrap_or(0);
            return;
        }

        self.load_ram_bytes(bytes);
    }
}
