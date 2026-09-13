use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_audio_discovery::gb_music::native::{GbBankedTiming, PreparedGbBanked};
use zeff_gb_core::{
    emulator::Emulator,
    hardware::types::hardware_mode::{HardwareMode, HardwareModePreference},
};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

const MAX_BOOT_FRAMES: usize = 120;
const MAX_EMPTY_AUDIO_FRAMES: usize = 4;

#[derive(Clone, Copy)]
enum CartridgeProfile {
    Banked,
    Musyx,
    Tose,
    QuickThunder,
    Ghx,
    SoundSystem,
    Carillon,
}

pub(crate) struct GbBankedSession {
    prepared: PreparedGbBanked,
    cartridge: CartridgeProfile,
    emulator: Emulator,
    options: RenderOptions,
    duration: usize,
    position: usize,
    mask: u16,
    pending: Vec<i16>,
    pending_offset: usize,
    floats: Vec<f32>,
    warnings: Vec<String>,
    boot_pending: bool,
}

impl GbBankedSession {
    pub(crate) fn new(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Banked,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_musyx(
        prepared: zeff_audio_discovery::gb_musyx::PreparedGbMusyx,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing: GbBankedTiming::Cgb,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Musyx,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_tose(
        prepared: zeff_audio_discovery::gb_tose::PreparedGbTose,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing: GbBankedTiming::Dmg,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Tose,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_quickthunder(
        prepared: zeff_audio_discovery::gb_quickthunder::PreparedGbQuickThunder,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_quickthunder::GbQuickThunderHardware::CgbDouble => {
                GbBankedTiming::CgbDouble
            }
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::QuickThunder,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_ghx(
        prepared: zeff_audio_discovery::gb_ghx::PreparedGbGhx,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_ghx::GbGhxHardware::CgbDouble => GbBankedTiming::CgbDouble,
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::Ghx,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_carillon(
        prepared: PreparedGbBanked,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        Self::new_inner(
            prepared,
            CartridgeProfile::Carillon,
            options,
            warnings,
            cancel,
        )
    }

    pub(crate) fn new_sound_system(
        prepared: zeff_audio_discovery::gb_sound_system::PreparedGbSoundSystem,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let timing = match prepared.hardware {
            zeff_audio_discovery::gb_sound_system::GbSoundSystemHardware::CgbNormal => {
                GbBankedTiming::Cgb
            }
            zeff_audio_discovery::gb_sound_system::GbSoundSystemHardware::CgbDouble => {
                GbBankedTiming::CgbDouble
            }
        };
        Self::new_inner(
            PreparedGbBanked {
                bytes: prepared.bytes,
                timing,
                ready_address: prepared.ready_address,
                ready_value: prepared.ready_value,
                ack_address: prepared.ack_address,
                ack_value: prepared.ack_value,
                wait_start: prepared.wait_start,
                wait_end: prepared.wait_end,
            },
            CartridgeProfile::SoundSystem,
            options,
            warnings,
            cancel,
        )
    }

    fn new_inner(
        prepared: PreparedGbBanked,
        cartridge: CartridgeProfile,
        options: RenderOptions,
        mut warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        let window = match cartridge {
            CartridgeProfile::Banked => 0xa0..0x100,
            CartridgeProfile::Musyx
            | CartridgeProfile::Tose
            | CartridgeProfile::QuickThunder
            | CartridgeProfile::Ghx
            | CartridgeProfile::SoundSystem
            | CartridgeProfile::Carillon => 0x150..0x200,
        };
        ensure!(
            prepared.wait_start >= window.start
                && prepared.wait_start < prepared.wait_end
                && prepared.wait_end <= window.end
                && (0xff80..0xffff).contains(&prepared.ready_address)
                && (0xff80..0xffff).contains(&prepared.ack_address)
                && prepared.ready_address != prepared.ack_address,
            "invalid Game Boy driver initialization handoff"
        );
        let emulator = Self::emulator(&prepared, cartridge, options.sample_rate)?;
        let requested = u64::from(options.max_seconds) * u64::from(options.sample_rate);
        let duration = usize::try_from(requested)?;
        ensure!(
            duration != 0,
            "Game Boy playback has no complete audio frames"
        );
        let timing = match prepared.timing {
            GbBankedTiming::Cgb => "CGB normal-speed",
            GbBankedTiming::CgbDouble => "CGB double-speed",
            GbBankedTiming::Dmg => "DMG",
        };
        warnings.push(format!("Runs the original sound driver in an isolated Game Boy emulator using {timing} timing; hardware-bit-exact output is not claimed."));
        warnings.push("Stops at the requested duration; automatic song-end and loop detection are unavailable. Native channels are mixed together.".to_owned());
        Ok(Self {
            prepared,
            cartridge,
            emulator,
            options,
            duration,
            position: 0,
            mask: 1,
            pending: Vec::new(),
            pending_offset: 0,
            floats: Vec::new(),
            warnings,
            boot_pending: true,
        })
    }

    fn emulator(
        prepared: &PreparedGbBanked,
        cartridge: CartridgeProfile,
        sample_rate: u32,
    ) -> Result<Emulator> {
        let cartridge_matches = match cartridge {
            CartridgeProfile::Banked => {
                prepared.bytes.len() == 0x20_0000
                    && prepared.bytes.get(0x147..0x149) == Some(&[0x10, 6])
                    && matches!(prepared.bytes.get(0x149), Some(3 | 5))
            }
            CartridgeProfile::Musyx => {
                matches!(prepared.bytes.get(0x147), Some(0x19..=0x1e))
                    && prepared.bytes.get(0x148).is_some_and(|&size| {
                        size <= 8 && prepared.bytes.len() == (0x8000_usize << size)
                    })
            }
            CartridgeProfile::Tose => {
                zeff_audio_discovery::gb_tose::supports_cartridge(&prepared.bytes)
            }
            CartridgeProfile::Ghx => {
                zeff_audio_discovery::gb_ghx::supports_cartridge(&prepared.bytes)
            }
            CartridgeProfile::SoundSystem => {
                zeff_audio_discovery::gb_sound_system::supports_cartridge(&prepared.bytes)
            }
            CartridgeProfile::Carillon => {
                zeff_audio_discovery::gb_carillon::supports_cartridge(&prepared.bytes)
            }
            CartridgeProfile::QuickThunder => {
                zeff_audio_discovery::gb_quickthunder::supports_cartridge(&prepared.bytes)
            }
        };
        let hardware_matches = match cartridge {
            CartridgeProfile::Tose => {
                prepared.timing == GbBankedTiming::Dmg
                    && !matches!(prepared.bytes.get(0x143), Some(0x80 | 0xc0))
            }
            CartridgeProfile::QuickThunder | CartridgeProfile::Ghx | CartridgeProfile::Carillon => {
                prepared.timing == GbBankedTiming::CgbDouble
                    && matches!(prepared.bytes.get(0x143), Some(0x80 | 0xc0))
            }
            CartridgeProfile::SoundSystem => {
                matches!(
                    prepared.timing,
                    GbBankedTiming::Cgb | GbBankedTiming::CgbDouble
                ) && matches!(prepared.bytes.get(0x143), Some(0x80 | 0xc0))
            }
            CartridgeProfile::Banked | CartridgeProfile::Musyx => {
                prepared.timing == GbBankedTiming::Cgb
                    && matches!(prepared.bytes.get(0x143), Some(0x80 | 0xc0))
            }
        };
        ensure!(
            cartridge_matches && hardware_matches,
            "Game Boy driver image does not match its cartridge profile"
        );
        let mode = match prepared.timing {
            GbBankedTiming::Cgb | GbBankedTiming::CgbDouble => HardwareModePreference::Auto,
            GbBankedTiming::Dmg => HardwareModePreference::ForceDmg,
        };
        let mut emulator = Emulator::from_rom_data(&prepared.bytes, mode)?;
        emulator.set_sample_rate(sample_rate);
        let initial_hardware = if prepared.timing == GbBankedTiming::CgbDouble {
            HardwareMode::CGBNormal
        } else {
            Self::hardware(prepared)
        };
        ensure!(
            emulator.hardware_mode() == initial_hardware,
            "Game Boy driver requires its qualified hardware mode"
        );
        Ok(emulator)
    }

    fn hardware(prepared: &PreparedGbBanked) -> HardwareMode {
        match prepared.timing {
            GbBankedTiming::Cgb => HardwareMode::CGBNormal,
            GbBankedTiming::CgbDouble => HardwareMode::CGBDouble,
            GbBankedTiming::Dmg => HardwareMode::DMG,
        }
    }

    fn step(&mut self, cancel: &AtomicBool) -> Result<()> {
        check_cancel(cancel)?;
        self.emulator.step_frame();
        check_cancel(cancel)?;
        ensure!(
            self.emulator.hardware_mode() == Self::hardware(&self.prepared),
            "Game Boy driver left its qualified timing"
        );
        self.floats.clear();
        self.emulator.drain_audio_samples_into(&mut self.floats);
        Ok(())
    }

    fn finish_boot(&mut self, cancel: &AtomicBool) -> Result<()> {
        if !self.boot_pending {
            return Ok(());
        }
        for _ in 0..MAX_BOOT_FRAMES {
            self.step(cancel)?;
            self.floats.clear();
            if self.emulator.cpu_peek8(self.prepared.ready_address) == self.prepared.ready_value
                && (self.prepared.wait_start..self.prepared.wait_end)
                    .contains(&self.emulator.cpu_pc())
            {
                self.emulator
                    .cpu_write8(self.prepared.ack_address, self.prepared.ack_value);
                self.boot_pending = false;
                return Ok(());
            }
        }
        anyhow::bail!("Game Boy sound driver did not reach its bounded initialization handoff")
    }
}

#[cfg(test)]
mod tests;

impl PcmSession for GbBankedSession {
    fn duration_frames(&self) -> usize {
        self.duration
    }
    fn position_frames(&self) -> usize {
        self.position
    }
    fn sample_rate(&self) -> u32 {
        self.options.sample_rate
    }
    fn track_count(&self) -> usize {
        1
    }
    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn reset(&mut self) -> Result<()> {
        self.emulator = Self::emulator(&self.prepared, self.cartridge, self.options.sample_rate)?;
        self.position = 0;
        self.pending.clear();
        self.pending_offset = 0;
        self.floats.clear();
        self.boot_pending = true;
        Ok(())
    }

    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(mask & !1 == 0, "track mask selects an unavailable track");
        self.mask = mask;
        Ok(())
    }

    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check_cancel(cancel)?;
        let requested = (output.len() / 2).min(self.duration - self.position) * 2;
        if requested == 0 {
            return Ok(0);
        }
        self.finish_boot(cancel)?;
        let mut written = 0;
        let mut empty_frames = 0;
        while written < requested {
            if self.pending_offset == self.pending.len() {
                self.step(cancel)?;
                ensure!(
                    self.floats.len().is_multiple_of(2)
                        && self.floats.iter().all(|sample| sample.is_finite()),
                    "Game Boy core returned invalid stereo audio"
                );
                self.pending.clear();
                self.pending.extend(
                    self.floats
                        .iter()
                        .map(|v| (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16),
                );
                self.pending_offset = 0;
                if self.pending.is_empty() {
                    empty_frames += 1;
                    ensure!(
                        empty_frames <= MAX_EMPTY_AUDIO_FRAMES,
                        "Game Boy core stopped producing audio"
                    );
                    continue;
                }
                empty_frames = 0;
            }
            let count = (requested - written).min(self.pending.len() - self.pending_offset);
            for index in 0..count {
                let sample = if self.mask == 0 {
                    0
                } else {
                    self.pending[self.pending_offset + index]
                };
                output[written + index] = fade(
                    sample,
                    self.position + (written + index) / 2,
                    self.duration,
                    self.options.sample_rate,
                    self.options.fade_seconds,
                );
            }
            self.pending_offset += count;
            written += count;
        }
        self.position += written / 2;
        Ok(written)
    }
}
