use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_audio_discovery::nes_native::{NesNativeTiming, PreparedNesNative};
use zeff_nes_core::{emulator::Emulator, hardware::cartridge::TimingMode};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

const MAX_BOOT_FRAMES: usize = 120;
const MAX_EMPTY_AUDIO_FRAMES: usize = 4;

pub(crate) struct NesSession {
    prepared: PreparedNesNative,
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

impl NesSession {
    pub(crate) fn new(
        prepared: PreparedNesNative,
        options: RenderOptions,
        mut warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        ensure!(
            prepared.wait_start >= 0x8000
                && prepared.wait_start < prepared.wait_end
                && prepared.wait_end <= 0xfffa
                && prepared.ready_address < 0x800
                && prepared.ack_address < 0x800
                && prepared.ready_address != prepared.ack_address,
            "invalid NES driver initialization handoff"
        );
        let emulator = Emulator::new(&prepared.bytes, f64::from(options.sample_rate))?;
        ensure!(
            emulator.cartridge_header().mapper_id == prepared.mapper
                && matches!(
                    (prepared.timing, emulator.cartridge_header().timing),
                    (NesNativeTiming::Ntsc, TimingMode::Ntsc)
                        | (NesNativeTiming::Pal, TimingMode::Pal)
                ),
            "NES driver image does not match its mapper or timing profile"
        );
        let region = match prepared.timing {
            NesNativeTiming::Ntsc => "NTSC",
            NesNativeTiming::Pal => "PAL",
        };
        warnings.push(format!("Runs the original sound driver in an isolated NES emulator using the reported {region} profile; hardware-bit-exact output is not claimed."));
        warnings.push("Records the requested duration, including any loops or silence. Individual native channels are mixed together.".to_owned());
        Ok(Self {
            prepared,
            emulator,
            options,
            duration: usize::from(options.max_seconds) * options.sample_rate as usize,
            position: 0,
            mask: 1,
            pending: Vec::new(),
            pending_offset: 0,
            floats: Vec::new(),
            warnings,
            boot_pending: true,
        })
    }

    fn step(&mut self, cancel: &AtomicBool) -> Result<()> {
        check_cancel(cancel)?;
        self.emulator.step_frame();
        check_cancel(cancel)?;
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
            if self.emulator.cpu_peek8(self.prepared.ready_address) == 1
                && (self.prepared.wait_start..self.prepared.wait_end)
                    .contains(&self.emulator.cpu_pc())
            {
                self.emulator.cpu_write8(self.prepared.ack_address, 1);
                self.boot_pending = false;
                return Ok(());
            }
        }
        anyhow::bail!("NES sound driver did not reach its bounded initialization handoff")
    }
}

impl PcmSession for NesSession {
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
        self.emulator = Emulator::new(&self.prepared.bytes, f64::from(self.options.sample_rate))?;
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
                    "NES core returned invalid stereo audio"
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
                        "NES core stopped producing audio"
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

#[cfg(test)]
mod tests;
