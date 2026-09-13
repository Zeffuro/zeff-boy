use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_gba_core::emulator::Emulator;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::{MAX_ROM_BYTES, gba_bootstrap, render::RenderOptions};

const GBA_HEADER_BYTES: usize = 0xc0;
const MAX_BOOT_FRAMES: usize = 900;
const MAX_EMPTY_AUDIO_FRAMES: usize = 4;
const ARM_PREFETCH_BYTES: u32 = 8;

#[cfg(test)]
mod tests;

pub(crate) struct GbaSession {
    rom: Vec<u8>,
    emulator: Emulator,
    options: RenderOptions,
    duration: usize,
    position: usize,
    mask: u16,
    pending: Vec<i16>,
    pending_offset: usize,
    floats: Vec<f32>,
    warnings: Vec<String>,
    wait_loop: Option<crate::audio_discovery::RomSpan>,
    boot_pending: bool,
}

impl GbaSession {
    pub(crate) fn new(
        rom: Vec<u8>,
        options: RenderOptions,
        mut warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        ensure!(
            (GBA_HEADER_BYTES..=MAX_ROM_BYTES).contains(&rom.len()),
            "invalid driver image size"
        );
        let emulator = Emulator::new(&rom, options.sample_rate)?;
        warnings.push("Runs the original sound driver in an isolated GBA emulator; hardware-bit-exact output is not claimed.".to_owned());
        warnings.push("Records the requested duration, including any loops or silence. Individual native channels are mixed together.".to_owned());
        Ok(Self {
            rom,
            emulator,
            options,
            duration: usize::from(options.max_seconds) * options.sample_rate as usize,
            position: 0,
            mask: 1,
            pending: Vec::new(),
            pending_offset: 0,
            floats: Vec::new(),
            warnings,
            wait_loop: None,
            boot_pending: false,
        })
    }

    pub(crate) fn new_ready(
        rom: Vec<u8>,
        wait_loop: crate::audio_discovery::RomSpan,
        options: RenderOptions,
        warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let mut session = Self::new(rom, options, warnings, cancel)?;
        session.wait_loop = Some(wait_loop);
        session.boot_pending = true;
        Ok(session)
    }

    fn finish_boot(&mut self, cancel: &AtomicBool) -> Result<()> {
        if !self.boot_pending {
            return Ok(());
        }
        let wait = self.wait_loop.expect("pending boot has a wait loop");
        for _ in 0..MAX_BOOT_FRAMES {
            check_cancel(cancel)?;
            self.emulator.step_frame();
            self.emulator.drain_audio_samples_into(&mut self.floats);
            self.floats.clear();
            let (_, iwram) = self.emulator.system_ram();
            let ready_offset = gba_bootstrap::READY_IWRAM_OFFSET;
            let ready = iwram.get(ready_offset..ready_offset + size_of::<u32>())
                == Some(&gba_bootstrap::READY_VALUE.to_le_bytes());
            let pc = self.emulator.cpu_registers()[15];
            if ready
                && (wait.canonical_cpu_address
                    ..wait.canonical_cpu_address + wait.byte_len + ARM_PREFETCH_BYTES)
                    .contains(&pc)
            {
                check_cancel(cancel)?;
                self.emulator
                    .cpu_write32(gba_bootstrap::ACK_ADDRESS, gba_bootstrap::ACK_VALUE);
                self.boot_pending = false;
                return Ok(());
            }
        }
        anyhow::bail!("sound driver did not reach its bounded initialization handoff")
    }
}

impl PcmSession for GbaSession {
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
        self.emulator = Emulator::new(&self.rom, self.options.sample_rate)?;
        self.position = 0;
        self.pending.clear();
        self.pending_offset = 0;
        self.floats.clear();
        self.boot_pending = self.wait_loop.is_some();
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
                check_cancel(cancel)?;
                self.emulator.step_frame();
                check_cancel(cancel)?;
                self.emulator.drain_audio_samples_into(&mut self.floats);
                ensure!(
                    self.floats.len().is_multiple_of(2),
                    "GBA core returned a partial audio frame"
                );
                ensure!(
                    self.floats.iter().all(|sample| sample.is_finite()),
                    "GBA core returned non-finite audio"
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
                        "GBA core stopped producing audio"
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
