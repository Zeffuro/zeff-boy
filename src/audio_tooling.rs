#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AudioChannelId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioVoiceClass {
    Pulse,
    Tone,
    Triangle,
    Noise,
    Pcm,
    Wavetable,
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

pub(crate) const GB_TEMPO_US_PER_BEAT: u32 = 1_004_520;
pub(crate) const NTSC_60_TEMPO_US_PER_BEAT: u32 = 998_340;
pub(crate) const WS_TEMPO_US_PER_BEAT: u32 = 795_018;

pub(crate) fn level_from_u4(volume: u8) -> f32 {
    f32::from(volume.min(15)) / 15.0
}
