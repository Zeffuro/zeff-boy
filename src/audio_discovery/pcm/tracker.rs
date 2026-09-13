use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Result, ensure};
use xmrs::prelude::Module;
use xmrsplayer::xmrsplayer::XmrsPlayer;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

self_cell::self_cell! {
    struct PlayerCell {
        owner: Arc<Module>,
        #[covariant]
        dependent: XmrsPlayer,
    }
}

pub(crate) struct TrackerSession {
    player: PlayerCell,
    options: RenderOptions,
    channels: usize,
    duration: usize,
    position: usize,
    mask: u16,
    ended: bool,
    warnings: Vec<String>,
}

impl TrackerSession {
    pub(crate) fn from_xm(
        bytes: &[u8],
        options: RenderOptions,
        mut warnings: Vec<String>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        ensure!(
            bytes.len() <= 128 * 1024 * 1024,
            "XM input exceeds the playback size limit"
        );
        let module = Module::load_xm(bytes).map_err(|error| {
            anyhow::anyhow!("could not load the validated XM projection: {error:?}")
        })?;
        check_cancel(cancel)?;
        let channels = module.get_num_channels();
        ensure!(
            channels <= 64,
            "XM channel count exceeds the playback limit"
        );
        let player = new_player(Arc::new(module), options.sample_rate);
        warnings.push("XM playback approximates the source driver's interpolation, envelopes, effects and mixing.".to_owned());
        warnings.push("Records the requested duration; XM order loops repeat and terminated songs leave silence.".to_owned());
        if channels > 16 {
            warnings.push(
                "This module has more than sixteen channels; preview exposes their combined mix."
                    .to_owned(),
            );
        }
        Ok(Self {
            player,
            options,
            channels,
            duration: usize::from(options.max_seconds) * options.sample_rate as usize,
            position: 0,
            mask: all_mask(channels),
            ended: false,
            warnings,
        })
    }
}

fn new_player(module: Arc<Module>, rate: u32) -> PlayerCell {
    PlayerCell::new(module, |module| {
        let mut player = XmrsPlayer::new(module, rate, 0);
        player.set_max_loop_count(0);
        player
    })
}

fn all_mask(channels: usize) -> u16 {
    match channels {
        1..=15 => (1 << channels) - 1,
        16 => u16::MAX,
        _ => 1,
    }
}

impl PcmSession for TrackerSession {
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
        if (1..=16).contains(&self.channels) {
            self.channels
        } else {
            1
        }
    }
    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn reset(&mut self) -> Result<()> {
        self.player = new_player(
            Arc::clone(self.player.borrow_owner()),
            self.options.sample_rate,
        );
        self.position = 0;
        self.ended = false;
        self.set_track_mask(self.mask)
    }

    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(
            mask & !all_mask(self.channels) == 0,
            "track mask selects an unavailable track"
        );
        self.mask = mask;
        self.player.with_dependent_mut(|_, player| {
            for channel in 0..self.channels {
                let selected = if self.channels > 16 {
                    mask != 0
                } else {
                    mask & (1 << channel) != 0
                };
                player.set_mute_channel(channel, !selected);
            }
        });
        Ok(())
    }

    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check_cancel(cancel)?;
        let frames = (output.len() / 2).min(self.duration - self.position);
        self.player.with_dependent_mut(|_, player| -> Result<()> {
            for (index, frame) in output[..frames * 2]
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .enumerate()
            {
                if index.is_multiple_of(256) {
                    check_cancel(cancel)?;
                }
                let sample = if self.ended {
                    None
                } else {
                    player.sample(true)
                };
                self.ended |= sample.is_none();
                let (left, right) = sample.unwrap_or((0, 0));
                frame[0] = fade(
                    left,
                    self.position + index,
                    self.duration,
                    self.options.sample_rate,
                    self.options.fade_seconds,
                );
                frame[1] = fade(
                    right,
                    self.position + index,
                    self.duration,
                    self.options.sample_rate,
                    self.options.fade_seconds,
                );
            }
            Ok(())
        })?;
        self.position += frames;
        Ok(frames * 2)
    }
}
