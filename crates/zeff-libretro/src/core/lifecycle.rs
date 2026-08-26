use super::{ActiveCore, CoreState};
use chrono::{Datelike, Local, Timelike};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_gba_core::hardware::cartridge::RtcDateTime;
use zeff_sega8_core::emulator::Sega8LoadConfig;
use zeff_sega8_core::hardware::cartridge::SystemHint;

impl CoreState {
    pub fn from_rom(data: &[u8], path: &str) -> anyhow::Result<Self> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let sample_rate = 48000u32;

        let core = match ext.as_str() {
            "pce" => {
                let host = zeff_pce_core::hardware::PceHuCardHost::new(data.to_vec(), sample_rate)?;
                ActiveCore::Pce(Box::new(host))
            }
            "gba" => {
                let mut emu = zeff_gba_core::emulator::Emulator::new(data, sample_rate)?;
                if emu.has_rtc()
                    && let Some(date_time) = local_rtc_date_time()
                {
                    emu.set_rtc_date_time(date_time);
                }
                ActiveCore::Gba(Box::new(emu))
            }
            "nes" => {
                let emu = zeff_nes_core::emulator::Emulator::new(data, sample_rate as f64)?;
                ActiveCore::Nes(Box::new(emu))
            }
            "ws" | "wsc" => {
                let emu = zeff_ws_core::emulator::Emulator::new(data, sample_rate)?;
                ActiveCore::Ws(Box::new(emu))
            }
            ext if SystemHint::from_extension(ext).is_some() => {
                let config = Sega8LoadConfig::from_path(sample_rate, std::path::Path::new(path));
                let emu = zeff_sega8_core::emulator::Emulator::new_with_config(data, config)?;
                ActiveCore::Sega8(Box::new(emu))
            }
            _ => {
                let pref = HardwareModePreference::Auto;
                let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(data, pref)?;
                emu.set_sample_rate(sample_rate);
                emu.set_sgb_border_enabled(false);
                ActiveCore::Gb(Box::new(emu))
            }
        };

        Ok(Self {
            core,
            rom_data: data.to_vec(),
            ram_cheats: Vec::new(),
            gba_codebreaker_state: zeff_emu_common::cheats::GbaCodeBreakerState::default(),
            audio_buf: Vec::with_capacity(4096),
            sample_rate,
            xrgb_buf: Vec::new(),
            rgb565_buf: Vec::new(),
            system_ram_buf: Vec::new(),
            video_ram_buf: Vec::new(),
            port_device: [crate::api::RETRO_DEVICE_JOYPAD; 2],
        })
    }

    pub fn reset(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(_) => {
                let pref = HardwareModePreference::Auto;
                if let Ok(mut emu) =
                    zeff_gb_core::emulator::Emulator::from_rom_data(&self.rom_data, pref)
                {
                    emu.set_sample_rate(self.sample_rate);
                    emu.set_sgb_border_enabled(false);
                    self.core = ActiveCore::Gb(Box::new(emu));
                }
            }
            ActiveCore::Gba(emu) => emu.reset(),
            ActiveCore::Nes(emu) => emu.reset(),
            ActiveCore::Pce(host) => host.reset(),
            ActiveCore::Sega8(emu) => emu.reset(),
            ActiveCore::Ws(emu) => emu.reset(),
        }
    }
}

fn local_rtc_date_time() -> Option<RtcDateTime> {
    let now = Local::now();
    RtcDateTime::new(
        u16::try_from(now.year()).ok()?,
        now.month() as u8,
        now.day() as u8,
        now.weekday().num_days_from_sunday() as u8,
        [now.hour() as u8, now.minute() as u8, now.second() as u8],
    )
    .ok()
}
