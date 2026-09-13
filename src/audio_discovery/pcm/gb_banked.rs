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

pub(crate) struct GbBankedSession {
    prepared: PreparedGbBanked,
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
        mut warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        ensure!(
            prepared.wait_start >= 0xa0
                && prepared.wait_start < prepared.wait_end
                && prepared.wait_end <= 0x100
                && (0xff80..0xffff).contains(&prepared.ready_address)
                && (0xff80..0xffff).contains(&prepared.ack_address)
                && prepared.ready_address != prepared.ack_address,
            "invalid Game Boy driver initialization handoff"
        );
        let emulator = Self::emulator(&prepared, options.sample_rate)?;
        let requested = u64::from(options.max_seconds) * u64::from(options.sample_rate);
        let duration = usize::try_from(requested)?;
        ensure!(
            duration != 0,
            "Game Boy playback has no complete audio frames"
        );
        warnings.push("Runs the original sound driver in an isolated Game Boy emulator using CGB normal-speed timing; hardware-bit-exact output is not claimed.".to_owned());
        warnings.push("Stops at the requested duration; automatic song-end and loop detection are unavailable. Native channels are mixed together.".to_owned());
        Ok(Self {
            prepared,
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

    fn emulator(prepared: &PreparedGbBanked, sample_rate: u32) -> Result<Emulator> {
        ensure!(
            prepared.bytes.len() == 0x20_0000
                && matches!(prepared.bytes.get(0x143), Some(0x80 | 0xc0))
                && prepared.bytes.get(0x147..0x149) == Some(&[0x10, 6])
                && matches!(prepared.bytes.get(0x149), Some(3 | 5)),
            "Game Boy driver image does not match its cartridge profile"
        );
        let mode = match prepared.timing {
            GbBankedTiming::Cgb => HardwareModePreference::Auto,
        };
        let mut emulator = Emulator::from_rom_data(&prepared.bytes, mode)?;
        emulator.set_sample_rate(sample_rate);
        ensure!(
            emulator.hardware_mode() == HardwareMode::CGBNormal,
            "Game Boy driver requires CGB normal-speed hardware"
        );
        Ok(emulator)
    }

    fn step(&mut self, cancel: &AtomicBool) -> Result<()> {
        check_cancel(cancel)?;
        self.emulator.step_frame();
        check_cancel(cancel)?;
        ensure!(
            self.emulator.hardware_mode() == HardwareMode::CGBNormal,
            "Game Boy driver left its qualified CGB normal-speed timing"
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
        self.emulator = Self::emulator(&self.prepared, self.options.sample_rate)?;
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
