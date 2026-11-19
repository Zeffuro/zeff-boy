mod sound;
mod sound_bus;
mod sound_clock;
mod sound_semantics;
use super::{ProtocolTarget, main_bus};
use anyhow::{Result, ensure};
pub(crate) use sound::{AudioEffect, SoundEvent, SoundOwner};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};

pub(crate) fn local_byte(
    memory: &crate::hardware::sound::SoundMemory,
    audio: &crate::hardware::audio::Audio,
    tick: u64,
    address: u16,
) -> anyhow::Result<u8> {
    sound_semantics::local_byte(memory, audio, tick, address)
}

pub(crate) struct AdvanceInput<'a> {
    pub half: u64,
    pub pass: u8,
    pub zclk: bool,
    pub response: super::board::SoundResponse,
    pub audio: &'a crate::hardware::audio::Audio,
    pub irq: bool,
}
