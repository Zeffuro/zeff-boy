use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use zeff_gba_core::emulator::Emulator;

use super::{NatsumeSong, NatsumeSongKind, PROFILE, validate_song};
use crate::audio_discovery::{
    MAX_ROM_BYTES,
    gsf::bootstrap,
    render::{MAX_DURATION_SECONDS, validate_sample_rate},
};

const ROM_BASE: u32 = 0x0800_0000;
pub(crate) const DEFAULT_DURATION_SECONDS: u16 = 180;

const REV0_CALLBACKS: bootstrap::Callbacks = bootstrap::Callbacks {
    init: 0x0800_062D,
    init_r0: None,
    select: 0x0805_4E71,
    main: 0x0805_36F5,
    prime_main_after_init: false,
    main_in_vblank: false,
    vsync: Some(0x0805_43C5),
    dma1: Some(0x0805_43E5),
    dma2: Some(0x0805_4429),
};

const REV1_CALLBACKS: bootstrap::Callbacks = bootstrap::Callbacks {
    init: 0x0800_058D,
    init_r0: Some(0x0300_2430),
    select: 0x0807_49F5,
    main: 0x0807_30E1,
    prime_main_after_init: true,
    main_in_vblank: true,
    vsync: Some(0x0807_3EDD),
    dma1: Some(0x0807_3EFD),
    dma2: Some(0x0807_3F41),
};

pub(crate) struct NativeRenderSession {
    patched_rom: Vec<u8>,
    emulator: Emulator,
    sample_rate: u32,
    duration_frames: usize,
    position_frames: usize,
    track_mask: u16,
    pending: Vec<i16>,
    pending_offset: usize,
    float_samples: Vec<f32>,
    warnings: Vec<String>,
}

pub(crate) fn prepare_rom(
    bytes: &[u8],
    song: &NatsumeSong,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    ensure!(
        song.kind == NatsumeSongKind::Music,
        "Natsume control entry has no music"
    );
    validate_song(bytes, song, cancel)?;
    let callbacks = match song.profile {
        PROFILE => REV0_CALLBACKS,
        "gba-natsume-driver-v1-rev1" => REV1_CALLBACKS,
        _ => anyhow::bail!("unsupported Natsume playback profile"),
    };
    build_rom(bytes, song.index, callbacks, cancel)
}

fn build_rom(
    bytes: &[u8],
    song_index: u16,
    callbacks: bootstrap::Callbacks,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    check_cancelled(cancel)?;
    ensure!(
        bytes.len() >= 0xC0 && bytes.len() < MAX_ROM_BYTES,
        "Natsume preview requires a bounded GBA cartridge with room for its bootstrap"
    );
    let base = bytes
        .len()
        .checked_add(3)
        .context("Natsume bootstrap offset overflow")?
        & !3;
    let code = bootstrap::build(ROM_BASE + base as u32, u32::from(song_index), callbacks)?;
    ensure!(
        base.checked_add(code.bytes.len())
            .is_some_and(|end| end <= MAX_ROM_BYTES),
        "Natsume bootstrap does not fit in cartridge memory"
    );
    let branch_words = (base as i64 - 8) / 4;
    ensure!(
        (-0x80_0000..0x80_0000).contains(&branch_words),
        "Natsume reset branch is out of range"
    );
    let branch = 0xEA00_0000 | (branch_words as u32 & 0x00FF_FFFF);
    let mut patched_rom = Vec::with_capacity(base + code.bytes.len());
    for block in bytes.chunks(64 * 1024) {
        check_cancelled(cancel)?;
        patched_rom.extend_from_slice(block);
    }
    patched_rom.resize(base, 0);
    patched_rom.extend_from_slice(&code.bytes);
    patched_rom[..4].copy_from_slice(&branch.to_le_bytes());
    check_cancelled(cancel)?;
    Ok(patched_rom)
}

impl NativeRenderSession {
    pub(crate) fn new(
        bytes: &[u8],
        song: &NatsumeSong,
        sample_rate: u32,
        max_seconds: u32,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancelled(cancel)?;
        ensure!(
            song.kind == NatsumeSongKind::Music,
            "this driver control entry produces no music; select a music entry to render"
        );
        validate_sample_rate(sample_rate)?;
        ensure!(
            (1..=u32::from(MAX_DURATION_SECONDS)).contains(&max_seconds),
            "maximum duration must be between 1 and {MAX_DURATION_SECONDS} seconds"
        );
        validate_song(bytes, song, cancel)?;
        let callbacks = match song.profile {
            PROFILE => REV0_CALLBACKS,
            "gba-natsume-driver-v1-rev1" => REV1_CALLBACKS,
            _ => anyhow::bail!("unsupported Natsume playback profile"),
        };
        Self::new_inner(
            bytes,
            song.index,
            sample_rate,
            max_seconds,
            callbacks,
            cancel,
        )
    }

    fn new_inner(
        bytes: &[u8],
        song_index: u16,
        sample_rate: u32,
        max_seconds: u32,
        callbacks: bootstrap::Callbacks,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let patched_rom = build_rom(bytes, song_index, callbacks, cancel)?;
        let emulator = Emulator::new(&patched_rom, sample_rate)?;
        let duration_frames = usize::try_from(u64::from(sample_rate) * u64::from(max_seconds))
            .context("Natsume preview duration overflows")?;
        Ok(Self {
            patched_rom,
            emulator,
            sample_rate,
            duration_frames,
            position_frames: 0,
            track_mask: 1,
            pending: Vec::new(),
            pending_offset: 0,
            float_samples: Vec::new(),
            warnings: Vec::new(),
        })
    }

    pub(crate) fn duration_frames(&self) -> usize {
        self.duration_frames
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn track_count(&self) -> usize {
        1
    }

    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn position_frames(&self) -> usize {
        self.position_frames
    }

    pub(crate) fn reset(&mut self) -> Result<()> {
        self.emulator = Emulator::new(&self.patched_rom, self.sample_rate)?;
        self.position_frames = 0;
        self.pending.clear();
        self.pending_offset = 0;
        self.float_samples.clear();
        Ok(())
    }

    pub(crate) fn set_track_mask(&mut self, track_mask: u16) -> Result<()> {
        ensure!(
            track_mask & !1 == 0,
            "track mask selects an unavailable track"
        );
        self.track_mask = track_mask;
        Ok(())
    }

    pub(crate) fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "render read buffer must contain complete stereo frames"
        );
        check_cancelled(cancel)?;
        let requested_frames = (output.len() / 2).min(self.duration_frames - self.position_frames);
        let mut written_samples = 0;
        let mut empty_frames = 0;
        while written_samples < requested_frames * 2 {
            if self.pending_offset == self.pending.len() {
                check_cancelled(cancel)?;
                self.emulator.step_frame();
                check_cancelled(cancel)?;
                self.emulator
                    .drain_audio_samples_into(&mut self.float_samples);
                ensure!(
                    self.float_samples.len().is_multiple_of(2),
                    "GBA core produced an incomplete stereo frame"
                );
                self.pending.clear();
                self.pending
                    .extend(self.float_samples.iter().copied().map(float_to_pcm));
                self.pending_offset = 0;
                if self.pending.is_empty() {
                    empty_frames += 1;
                    ensure!(
                        empty_frames <= 4,
                        "Natsume preview core stopped producing audio"
                    );
                    continue;
                }
                empty_frames = 0;
            }
            let count = (requested_frames * 2 - written_samples)
                .min(self.pending.len() - self.pending_offset);
            let target = &mut output[written_samples..written_samples + count];
            if self.track_mask == 0 {
                target.fill(0);
            } else {
                target.copy_from_slice(
                    &self.pending[self.pending_offset..self.pending_offset + count],
                );
            }
            self.pending_offset += count;
            written_samples += count;
        }
        self.position_frames += written_samples / 2;
        Ok(written_samples)
    }
}

fn float_to_pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Natsume preview cancelled");
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn store(rom: &mut [u8], at: usize, words: &[u32]) {
        for (slot, value) in rom[at..].as_chunks_mut::<4>().0.iter_mut().zip(words) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }

    fn fixture() -> Vec<u8> {
        let mut rom = vec![0; 0x1000];
        rom[0xB2] = 0x96;
        store(
            &mut rom,
            0x200,
            &[
                0xE59F_1020,
                0xE581_0000,
                0xE3A0_2001,
                0xE581_2004,
                0xE59F_1014,
                0xE3A0_0080,
                0xE5C1_0084,
                0xE59F_000C,
                0xE581_0080,
                0xE12F_FF1E,
                0x0200_0000,
                0x0400_0000,
                0x0000_1177,
            ],
        );
        store(
            &mut rom,
            0x240,
            &[0xE59F_1004, 0xE581_000C, 0xE12F_FF1E, 0x0200_0000],
        );
        store(
            &mut rom,
            0x280,
            &[
                0xE59F_1020,
                0xE591_0008,
                0xE280_0001,
                0xE581_0008,
                0xE59F_1014,
                0xE59F_0014,
                0xE581_0060,
                0xE59F_0010,
                0xE581_0064,
                0xE12F_FF1E,
                0x0200_0000,
                0x0400_0000,
                0xF080_0000,
                0x0000_8400,
            ],
        );
        rom
    }

    fn session(cancel: &AtomicBool) -> Result<NativeRenderSession> {
        session_at(44_100, 1, cancel)
    }

    pub(crate) fn session_at(
        sample_rate: u32,
        seconds: u32,
        cancel: &AtomicBool,
    ) -> Result<NativeRenderSession> {
        NativeRenderSession::new_inner(
            &fixture(),
            37,
            sample_rate,
            seconds,
            bootstrap::Callbacks {
                init: 0x0800_0200,
                init_r0: Some(0x0300_2430),
                select: 0x0800_0240,
                main: 0x0800_0280,
                prime_main_after_init: false,
                main_in_vblank: false,
                vsync: None,
                dma1: None,
                dma2: None,
            },
            cancel,
        )
    }

    #[test]
    fn synthetic_driver_renders_and_reset_is_deterministic() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let mut renderer = session(&cancel)?;
        assert_eq!(renderer.duration_frames(), 44_100);
        assert_eq!(renderer.sample_rate(), 44_100);
        assert_eq!(renderer.track_count(), 1);
        assert_eq!(renderer.position_frames(), 0);
        assert!(renderer.warnings().is_empty());

        let mut first = vec![0; 8_192];
        assert_eq!(renderer.read(&mut first, &cancel)?, first.len());
        assert!(first.iter().any(|sample| *sample != 0));
        assert_eq!(renderer.position_frames(), first.len() / 2);
        assert_eq!(renderer.emulator.cpu_peek32(0x0200_0000), 0x0300_2430);
        assert_eq!(renderer.emulator.cpu_peek32(0x0200_0004), 1);
        assert!(renderer.emulator.cpu_peek32(0x0200_0008) > 0);
        assert_eq!(renderer.emulator.cpu_peek32(0x0200_000C), 37);

        renderer.reset()?;
        let mut replay = vec![0; first.len()];
        renderer.read(&mut replay, &cancel)?;
        assert_eq!(replay, first);
        Ok(())
    }

    #[test]
    fn mixed_track_mask_duration_and_cancellation_are_bounded() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let mut renderer = session(&cancel)?;
        renderer.set_track_mask(0)?;
        let mut muted = vec![1; 2_000];
        assert_eq!(renderer.read(&mut muted, &cancel)?, muted.len());
        assert!(muted.iter().all(|sample| *sample == 0));
        assert_eq!(renderer.position_frames(), 1_000);
        assert!(renderer.set_track_mask(2).is_err());

        renderer.set_track_mask(1)?;
        renderer.reset()?;
        let mut output = vec![0; 100_000];
        assert_eq!(renderer.read(&mut output, &cancel)?, 88_200);
        assert_eq!(renderer.position_frames(), renderer.duration_frames());
        assert_eq!(renderer.read(&mut output, &cancel)?, 0);

        renderer.reset()?;
        cancel.store(true, Ordering::Relaxed);
        assert!(renderer.read(&mut output, &cancel).is_err());
        assert_eq!(renderer.position_frames(), 0);
        Ok(())
    }
}
