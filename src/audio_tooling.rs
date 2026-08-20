#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct AudioChannelId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioVoiceClass {
    Pulse,
    Tone,
    Triangle,
    Noise,
    Pcm,
    Wavetable,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioSemanticCaps(u16);

impl AudioSemanticCaps {
    pub(crate) const GATE: Self = Self(1 << 0);
    pub(crate) const PITCH: Self = Self(1 << 1);
    pub(crate) const LEVEL: Self = Self(1 << 2);

    pub(crate) const GATE_LEVEL: Self = Self(Self::GATE.0 | Self::LEVEL.0);
    pub(crate) const GATE_PITCH_LEVEL: Self = Self(Self::GATE.0 | Self::PITCH.0 | Self::LEVEL.0);

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioChannelDescriptor {
    pub(crate) id: AudioChannelId,
    pub(crate) name: &'static str,
    pub(crate) group: &'static str,
    pub(crate) class: AudioVoiceClass,
    pub(crate) caps: AudioSemanticCaps,
    pub(crate) muteable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioTopology {
    pub(crate) generation: u32,
    pub(crate) channels: &'static [AudioChannelDescriptor],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioRecordingContext {
    pub(crate) system: System,
    pub(crate) topology: AudioTopology,
    pub(crate) clock_rate: ClockRate,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioVoiceState {
    pub(crate) channel: AudioChannelId,
    pub(crate) name: &'static str,
    pub(crate) class: AudioVoiceClass,
    pub(crate) active: bool,
    pub(crate) pitch_hz: Option<f64>,
    pub(crate) level: Option<f32>,
}

impl AudioVoiceState {
    pub(crate) fn level_velocity(&self) -> u8 {
        let level = self.level.unwrap_or(1.0).clamp(0.0, 1.0);
        (level.mul_add(127.0, 0.5).floor() as u8).min(127)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioSemanticFrame {
    pub(crate) frame: u64,
    pub(crate) tempo_us_per_beat: u32,
    pub(crate) voices: Vec<AudioVoiceState>,
}

pub(crate) fn debug_assert_frame_matches_topology(
    topology: AudioTopology,
    frame: &AudioSemanticFrame,
) {
    debug_assert!(topology.generation > 0);
    for (index, channel) in topology.channels.iter().enumerate() {
        debug_assert!(!channel.name.is_empty());
        debug_assert!(!channel.group.is_empty());
        debug_assert!(
            !topology.channels[..index]
                .iter()
                .any(|previous| previous.id == channel.id)
        );
        let _ = (channel.class, channel.muteable);
    }
    for voice in &frame.voices {
        let channel = topology
            .channels
            .iter()
            .find(|channel| channel.id == voice.channel);
        debug_assert!(channel.is_some());
        let Some(channel) = channel else {
            continue;
        };
        debug_assert!(channel.caps.contains(AudioSemanticCaps::GATE));
        debug_assert!(channel.caps.contains(AudioSemanticCaps::LEVEL) || voice.level.is_none());
        debug_assert!(channel.caps.contains(AudioSemanticCaps::PITCH) || voice.pitch_hz.is_none());
    }
}

pub(crate) const GB_TEMPO_US_PER_BEAT: u32 = 1_004_520;
pub(crate) const NTSC_60_TEMPO_US_PER_BEAT: u32 = 998_340;
pub(crate) const WS_TEMPO_US_PER_BEAT: u32 = 795_018;

pub(crate) fn level_from_u4(volume: u8) -> f32 {
    f32::from(volume.min(15)) / 15.0
}
use zeff_emu_common::system::System;
use zeff_emu_common::time::ClockRate;
