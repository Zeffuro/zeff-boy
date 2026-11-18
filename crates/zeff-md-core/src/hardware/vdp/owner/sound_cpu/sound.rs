#[path = "sound_audio.rs"]
mod audio;
#[path = "sound_branch.rs"]
mod branch;
#[path = "sound_copy.rs"]
mod copy;
#[path = "sound_djnz.rs"]
mod djnz;
#[path = "sound_ed_word.rs"]
mod ed_word;
#[path = "sound_event.rs"]
mod event;
#[path = "sound_indexed.rs"]
mod indexed;
#[path = "sound_m1.rs"]
mod m1;
#[path = "sound_memory.rs"]
mod rollback_memory;
#[path = "sound_runtime.rs"]
mod runtime;
#[path = "sound_word.rs"]
mod word;
use copy::CopyHistory;
#[path = "sound_state.rs"]
mod state;
pub(crate) use audio::AudioEffect;
use m1::M1History;
#[path = "sound_reset.rs"]
mod reset;
use super::super::board::{ProtocolTarget, PsgWrite, SoundPins, SoundResponse};
use super::AdvanceInput;
use super::sound_bus::{Phase, SoundBus};
use super::sound_clock::{Edge, SoundClock};
use super::sound_semantics::{Plan, Shape, local_byte};
use crate::hardware::{audio::Audio, sound::SoundMemory};
use anyhow::{Result, ensure};
pub(crate) use event::SoundEvent;
use reset::SoundReset;
use std::sync::Arc;
use zeff_m68k::bus::ByteLanes;
use zeff_z80::{BusState, Cpu, DjnzTail, JrTail, WordAddTail};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Semantic {
    cpu: Cpu,
    state: BusState,
    memory: SoundMemory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExternalReceipt {
    address: u16,
    bank: u16,
    write: bool,
    value: u8,
    half: u64,
    waits: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SoundOwner {
    clock: SoundClock,
    bus: SoundBus,
    semantic: Arc<Semantic>,
    effects: Vec<AudioEffect>,
    released: bool,
    completed: [u64; 2],
    last_external: Option<ExternalReceipt>,
    request_low: bool,
    wait_high: bool,
    irq_low: bool,
    boundary_irq: bool,
    reset: Option<SoundReset>,
    stable_grant: bool,
    m1_history: M1History,
    refresh_hold: bool,
    data_hold: Option<u8>,
    jr_tail: Option<JrTail>,
    word_tail: Option<WordAddTail>,
    djnz_tail: Option<DjnzTail>,
    copy_history: CopyHistory,
}

impl SoundOwner {
    pub fn software_origin() -> Self {
        Self {
            clock: SoundClock::software_origin(),
            bus: SoundBus::software_origin(),
            semantic: Arc::new(Semantic {
                cpu: Cpu::new_with_memptr_profile(
                    zeff_z80::ResetState::new(zeff_z80::Z80_RESET_PC, zeff_z80::Z80_RESET_SP),
                    zeff_z80::MemptrProfile::Nmos,
                ),
                state: BusState::default(),
                memory: SoundMemory::default(),
            }),
            effects: Vec::new(),
            released: false,
            completed: [0; 2],
            last_external: None,
            request_low: false,
            wait_high: false,
            irq_low: false,
            boundary_irq: false,
            reset: Some(SoundReset::held_origin()),
            stable_grant: false,
            m1_history: M1History::origin(),
            refresh_hold: false,
            data_hold: None,
            jr_tail: None,
            word_tail: None,
            djnz_tail: None,
            copy_history: CopyHistory::Unknown,
        }
    }
    pub fn cpu(&self) -> &Cpu {
        &self.semantic.cpu
    }
    pub fn bus_state(&self) -> BusState {
        self.semantic.state
    }
    pub fn memory(&self) -> &SoundMemory {
        &self.semantic.memory
    }
    pub fn bank(&self) -> u16 {
        self.semantic.memory.bank
    }
    pub fn half(&self) -> u64 {
        self.clock.half()
    }
    pub fn frontier(&self) -> (u64, u8, bool) {
        (self.clock.half(), self.clock.pass(), self.clock.high())
    }
    pub fn pins(&self) -> SoundPins {
        if let Some(reset) = &self.reset {
            if reset.waiting_repeat() {
                return self.bus.pins(false);
            }
            reset.pins()
        } else {
            let mut pins = self.bus.pins(false);
            pins.refresh = self.refresh_hold;
            pins.data = self.data_hold;
            pins
        }
    }
    pub fn observed_controls(&self) -> (bool, bool) {
        (
            self.semantic.memory.requested,
            self.semantic.memory.reset_asserted,
        )
    }
    pub fn take_audio_effects(&mut self) -> Vec<AudioEffect> {
        std::mem::take(&mut self.effects)
    }

    pub fn enqueue_psg(&mut self, receipt: PsgWrite) -> Result<()> {
        receipt.validate()?;
        ensure!(
            (receipt.half, receipt.pass) == (self.clock.half(), self.clock.pass()),
            "PSG write receipt differs from SOUND frontier"
        );
        ensure!(self.effects.len() < 4, "physical SOUND effect capacity");
        self.effects.push(AudioEffect::Psg {
            tick: receipt.half.div_ceil(2),
            value: receipt.value,
        });
        Ok(())
    }

    fn sync_grant(&mut self) {
        let memory = &self.semantic.memory;
        let granted = self.pins().bus_ack && memory.requested && !memory.reset_asserted;
        if memory.granted != granted {
            Arc::make_mut(&mut self.semantic).memory.granted = granted;
        }
    }

    fn local_write(&mut self, address: u16, value: u8, tick: u64) -> Result<()> {
        if (0x4000..=0x5fff).contains(&address) {
            self.effects.push(AudioEffect::Opn2 {
                tick,
                port: (address & 3) as u8,
                value,
            });
        } else {
            ensure!(
                Arc::make_mut(&mut self.semantic)
                    .memory
                    .write(address, value),
                "unowned local SOUND write {address:04x}"
            );
        }
        Ok(())
    }

    pub fn apply_main_event(&mut self, event: SoundEvent) -> Result<()> {
        let mut next = self.clone();
        next.apply_event(event)?;
        *self = next;
        Ok(())
    }

    fn apply_event(&mut self, event: SoundEvent) -> Result<()> {
        event.validate()?;
        ensure!(
            event.tick >= self.clock.half() / 2 && event.tick <= self.clock.half().div_ceil(2) + 1,
            "SOUND MAIN event frontier differs"
        );
        let super::super::main_bus::Operation::Write(word) = event.request.operation else {
            anyhow::bail!("SOUND event is not a write")
        };
        let lanes = event.request.access.lanes;
        let address = event.request.access.bus_address;
        match event.target {
            ProtocolTarget::SoundControl => {
                if lanes == ByteLanes::Lower {
                    return Ok(());
                }
                ensure!(
                    matches!(address & 0xffff00, 0xa11100 | 0xa11200),
                    "unowned SOUND control address"
                );
            }
            ProtocolTarget::SoundRam => {
                if self.semantic.memory.reset_asserted || !self.pins().bus_ack {
                    return Ok(());
                }
                ensure!(
                    matches!(self.bus.phase, Phase::Granted | Phase::ReturnHigh),
                    "SOUND aperture write without physical BUSACK"
                );
                let address = ((address + u32::from(lanes == ByteLanes::Lower)) & 0x7fff) as u16;
                let value = if lanes == ByteLanes::Lower {
                    word as u8
                } else {
                    (word >> 8) as u8
                };
                if (0x7f10..=0x7f17).contains(&address) {
                    if address & 1 != 0 {
                        self.effects.push(AudioEffect::Psg {
                            tick: event.tick,
                            value,
                        });
                    }
                } else {
                    self.local_write(address, value, event.tick)?;
                }
            }
            ProtocolTarget::Psg => {
                if lanes != ByteLanes::Upper {
                    self.effects.push(AudioEffect::Psg {
                        tick: event.tick,
                        value: word as u8,
                    });
                }
            }
            _ => anyhow::bail!("non-SOUND event sent to SOUND owner"),
        }
        ensure!(self.effects.len() <= 4, "physical SOUND effect capacity");
        Ok(())
    }
}
