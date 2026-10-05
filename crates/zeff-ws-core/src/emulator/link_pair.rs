use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

use super::Emulator;
use crate::hardware::constants::CYCLES_PER_FRAME;

mod state;
#[cfg(test)]
mod tests;

pub const WS11_INPUT_MASK: u16 = 0x07ff;
pub const MAX_CABLE_EVENTS: usize = 256;
const MAX_STEPS: usize = 200_000;
const MAX_BUFFERED_SAMPLES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Endpoint {
    Zero,
    One,
}

impl Endpoint {
    pub fn index(self) -> usize {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CableEvent {
    tick: u64,
    sender: usize,
    generation: u64,
    byte: u8,
    baud_bps: u32,
}

#[derive(Clone)]
struct Schedule {
    epochs: [u64; 2],
    events: Vec<CableEvent>,
    frame: u64,
}

pub struct WonderSwanLinkPair {
    owner: Arc<()>,
    schedule: Schedule,
    expected: [[u8; 32]; 2],
    media: [[u8; 32]; 2],
    identities: [Arc<()>; 2],
    media_rom: [Arc<[u8]>; 2],
    valid: bool,
}

#[derive(Clone)]
pub struct WonderSwanPairSnapshot {
    owner: Weak<()>,
    machines: [Emulator; 2],
    schedule: Schedule,
    checksum: [u8; 32],
    retained_bytes: usize,
    shared_media_bytes: usize,
}

pub struct PairFrame {
    pub frame: u64,
    pub audio: [Vec<f32>; 2],
    pub bus_ticks: [u64; 2],
}

impl WonderSwanLinkPair {
    pub fn new(machines: [&Emulator; 2]) -> Result<Self> {
        ensure!(
            !Arc::ptr_eq(&machines[0].link_identity, &machines[1].link_identity),
            "paired endpoints require distinct machine contexts"
        );
        for machine in machines {
            state::validate(machine)?;
            ensure!(
                <[u8; 32]>::from(Sha256::digest(machine.cartridge_rom_bytes()))
                    == machine.rom_hash(),
                "paired media payload differs from its identity"
            );
            ensure!(
                machine.uart_debug_snapshot().completed_tx_count == 0,
                "paired link cannot adopt completed bytes from an earlier cable"
            );
        }
        Ok(Self {
            owner: Arc::new(()),
            schedule: Schedule {
                epochs: machines.map(Emulator::bus_cycles),
                events: Vec::new(),
                frame: 0,
            },
            expected: state::machine_hashes(machines)?,
            media: machines.map(Emulator::rom_hash),
            identities: machines.map(|machine| machine.link_identity.clone()),
            media_rom: machines.map(|machine| machine.bus.cartridge.shared_rom()),
            valid: true,
        })
    }

    pub fn frame(&self) -> u64 {
        self.schedule.frame
    }

    pub fn pending_events(&self) -> usize {
        self.schedule.events.len()
    }

    pub fn bus_ticks(&self, machines: [&Emulator; 2]) -> Result<[u64; 2]> {
        self.check(machines)?;
        self.ticks(machines)
    }

    pub fn video_checksum(&self, machines: [&Emulator; 2]) -> Result<[u8; 32]> {
        self.check(machines)?;
        let mut hash = Sha256::new();
        hash.update(b"zeff-ws-pair-video-v1\0");
        for machine in machines {
            hash.update(machine.framebuffer());
        }
        Ok(hash.finalize().into())
    }

    pub fn capture(&self, machines: [&Emulator; 2]) -> Result<WonderSwanPairSnapshot> {
        self.check(machines)?;
        let checksum = state::pair_hash(machines, &self.schedule)?;
        let shared_media_bytes = machines[0].cartridge_rom_bytes().len()
            + if std::ptr::eq(
                machines[0].cartridge_rom_bytes().as_ptr(),
                machines[1].cartridge_rom_bytes().as_ptr(),
            ) {
                0
            } else {
                machines[1].cartridge_rom_bytes().len()
            };
        let retained_bytes =
            machines.iter().try_fold(
                std::mem::size_of::<WonderSwanPairSnapshot>(),
                |total, machine| -> Result<usize> {
                    Ok(total
                        + machine.encode_state()?.len()
                        + machine.bus.apu.rollback_runtime_bytes())
                },
            )? + shared_media_bytes
                + self.schedule.events.capacity() * std::mem::size_of::<CableEvent>();
        Ok(WonderSwanPairSnapshot {
            owner: Arc::downgrade(&self.owner),
            machines: machines.map(Clone::clone),
            schedule: self.schedule.clone(),
            checksum,
            retained_bytes,
            shared_media_bytes,
        })
    }

    pub fn restore(
        &mut self,
        machines: [&mut Emulator; 2],
        snapshot: &WonderSwanPairSnapshot,
    ) -> Result<()> {
        self.check_context([&*machines[0], &*machines[1]])?;
        ensure!(
            snapshot.owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "foreign paired snapshot"
        );
        let saved = [&snapshot.machines[0], &snapshot.machines[1]];
        ensure!(
            (0..2).all(|i| Arc::ptr_eq(&saved[i].link_identity, &self.identities[i]))
                && snapshot.schedule.epochs == self.schedule.epochs,
            "paired snapshot machine context differs"
        );
        self.check_media(saved)?;
        ensure!(
            saved.map(Emulator::rom_hash) == self.media,
            "paired snapshot media differs"
        );
        ensure!(
            state::pair_hash(saved, &snapshot.schedule)? == snapshot.checksum,
            "paired snapshot checkpoint differs"
        );
        for machine in saved {
            state::validate(machine)?;
        }
        let expected = state::machine_hashes(saved)?;
        let restored = snapshot.machines.clone();
        let [left, right] = restored;
        *machines[0] = left;
        *machines[1] = right;
        self.schedule = snapshot.schedule.clone();
        self.expected = expected;
        self.valid = true;
        Ok(())
    }

    /// Bits 0..3 are X1..X4, 4..7 Y1..Y4, and 8..10 A, B, Start.
    pub fn advance_frame(
        &mut self,
        mut machines: [&mut Emulator; 2],
        inputs: [u16; 2],
    ) -> Result<PairFrame> {
        self.check([&*machines[0], &*machines[1]])?;
        ensure!(
            inputs.iter().all(|input| input & !WS11_INPUT_MASK == 0),
            "invalid WS11 input"
        );
        let frame = self
            .schedule
            .frame
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("paired frame overflow"))?;
        let target = frame
            .checked_mul(u64::from(CYCLES_PER_FRAME))
            .ok_or_else(|| anyhow::anyhow!("paired bus horizon overflow"))?;
        for (machine, input) in machines.iter_mut().zip(inputs) {
            machine.bus.end_frame_service();
            let buttons =
                (input as u8 & 0xf0) | ((input >> 8) as u8 & 3) | (((input >> 10) as u8 & 1) << 3);
            machine.set_input(buttons, input as u8 & 0x0f);
            machine.clear_frame_ready();
        }
        let result = self.run(&mut machines, [target; 2]);
        if let Err(error) = result {
            self.retire([&*machines[0], &*machines[1]])?;
            return Err(error);
        }
        let mut audio = [Vec::new(), Vec::new()];
        for (machine, samples) in machines.iter_mut().zip(&mut audio) {
            machine.frame_count = machine.frame_count.wrapping_add(1);
            machine.drain_audio_samples_into(samples);
        }
        self.schedule.frame = frame;
        self.expected = state::machine_hashes([&*machines[0], &*machines[1]])?;
        Ok(PairFrame {
            frame,
            audio,
            bus_ticks: self.ticks([&*machines[0], &*machines[1]])?,
        })
    }

    pub fn advance_to(&mut self, mut machines: [&mut Emulator; 2], tick: u64) -> Result<()> {
        self.check([&*machines[0], &*machines[1]])?;
        let now = self.ticks([&*machines[0], &*machines[1]])?;
        ensure!(
            now.iter()
                .all(|&now| tick >= now && tick - now <= u64::from(CYCLES_PER_FRAME) * 2),
            "paired tick horizon is outside the bounded window"
        );
        for machine in &mut machines {
            machine.bus.end_frame_service();
        }
        let result = self.run(&mut machines, [tick; 2]);
        if result.is_err() {
            self.valid = false;
        }
        self.expected = state::machine_hashes([&*machines[0], &*machines[1]])?;
        result
    }

    fn check_context(&self, machines: [&Emulator; 2]) -> Result<()> {
        ensure!(
            (0..2).all(|i| Arc::ptr_eq(&machines[i].link_identity, &self.identities[i])),
            "foreign paired machine context"
        );
        self.check_media(machines)?;
        ensure!(
            machines.map(Emulator::rom_hash) == self.media,
            "foreign paired machines"
        );
        ensure!(
            state::machine_hashes(machines)? == self.expected,
            "paired machines were mutated outside the scheduler"
        );
        for machine in machines {
            state::validate_controls(machine)?;
        }
        Ok(())
    }

    fn check_media(&self, machines: [&Emulator; 2]) -> Result<()> {
        ensure!(
            (0..2)
                .all(|i| Arc::ptr_eq(&machines[i].bus.cartridge.shared_rom(), &self.media_rom[i])),
            "paired immutable media context differs"
        );
        Ok(())
    }

    fn check(&self, machines: [&Emulator; 2]) -> Result<()> {
        ensure!(self.valid, "paired scheduler is retired");
        self.check_context(machines)?;
        for machine in machines {
            state::validate(machine)?;
        }
        Ok(())
    }

    fn retire(&mut self, machines: [&Emulator; 2]) -> Result<()> {
        self.valid = false;
        self.expected = state::machine_hashes(machines)?;
        Ok(())
    }

    fn ticks(&self, machines: [&Emulator; 2]) -> Result<[u64; 2]> {
        let mut ticks = [0; 2];
        for i in 0..2 {
            ticks[i] = machines[i]
                .bus_cycles()
                .checked_sub(self.schedule.epochs[i])
                .ok_or_else(|| anyhow::anyhow!("paired bus clock moved before its epoch"))?;
        }
        Ok(ticks)
    }

    fn collect(&mut self, machines: &mut [&mut Emulator; 2]) -> Result<()> {
        for (sender, machine) in machines.iter_mut().enumerate() {
            while let Some(event) = machine.take_wonder_swan_link_tx_event() {
                ensure!(
                    self.schedule.events.len() < MAX_CABLE_EVENTS,
                    "paired cable queue overflow"
                );
                let tick = event
                    .completed_cycle
                    .checked_sub(self.schedule.epochs[sender])
                    .ok_or_else(|| anyhow::anyhow!("paired TX predates its bus epoch"))?;
                self.schedule.events.push(CableEvent {
                    tick,
                    sender,
                    generation: event.generation,
                    byte: event.byte,
                    baud_bps: event.baud_bps,
                });
            }
        }
        self.schedule
            .events
            .sort_by_key(|event| (event.tick, event.sender, event.generation));
        Ok(())
    }

    fn deliver(&mut self, machines: &mut [&mut Emulator; 2]) -> Result<()> {
        let ticks = self.ticks([&*machines[0], &*machines[1]])?;
        let watermark = ticks[0].min(ticks[1]);
        while self
            .schedule
            .events
            .first()
            .is_some_and(|event| event.tick <= watermark)
        {
            let event = self.schedule.events.remove(0);
            machines[1 - event.sender].receive_wonder_swan_link_byte(event.byte);
        }
        Ok(())
    }

    fn run(&mut self, machines: &mut [&mut Emulator; 2], limits: [u64; 2]) -> Result<()> {
        self.collect(machines)?;
        for _ in 0..MAX_STEPS {
            self.deliver(machines)?;
            ensure!(
                machines
                    .iter()
                    .all(|machine| machine.apu_debug_snapshot().buffered_samples
                        <= MAX_BUFFERED_SAMPLES),
                "paired audio buffer budget exhausted"
            );
            let ticks = self.ticks([&*machines[0], &*machines[1]])?;
            let done = std::array::from_fn::<_, 2, _>(|i| ticks[i] >= limits[i]);
            if done == [true, true] {
                return Ok(());
            }
            // Instructions crossing an event finish with pre-RX state; RX is visible at the next boundary.
            let i = if done[0] || (!done[1] && ticks[1] < ticks[0]) {
                1
            } else {
                0
            };
            let before = machines[i].bus_cycles();
            let can_halt = |machine: &Emulator| {
                machine.cpu.can_fast_forward_halt() && !machine.bus.has_pending_interrupt_signal()
            };
            if !done[0]
                && !done[1]
                && ticks[0] == ticks[1]
                && can_halt(machines[0])
                && can_halt(machines[1])
            {
                let mut advance = u64::from(
                    machines[0]
                        .bus
                        .halted_cpu_next_event_cycles()
                        .min(machines[1].bus.halted_cpu_next_event_cycles()),
                );
                advance = advance.min(limits[0] - ticks[0]).min(limits[1] - ticks[1]);
                if let Some(event) = self.schedule.events.first() {
                    advance = advance.min(event.tick.saturating_sub(ticks[0]));
                }
                ensure!(advance != 0, "paired halt made no progress");
                for machine in machines.iter_mut() {
                    machine
                        .cpu
                        .advance_halted_cycles(&mut machine.bus, advance as u32);
                }
            } else if can_halt(machines[i]) {
                let mut advance = u64::from(machines[i].bus.halted_cpu_next_event_cycles())
                    .min(limits[i].saturating_sub(ticks[i]));
                advance = advance.min(ticks[1 - i].saturating_sub(ticks[i]).max(1));
                if let Some(event) = self.schedule.events.iter().find(|event| event.sender != i) {
                    advance = advance.min(event.tick.saturating_sub(ticks[i]).max(1));
                }
                ensure!(advance != 0, "paired halt made no progress");
                machines[i]
                    .cpu
                    .advance_halted_cycles(&mut machines[i].bus, advance as u32);
            } else {
                machines[i].step_instruction();
            }
            ensure!(
                machines[i].bus_cycles() > before,
                "paired instruction made no bus progress"
            );
            ensure!(
                machines
                    .iter()
                    .all(|machine| !machine.is_cpu_suspended() && machine.last_trap().is_none()),
                "paired execution stopped or trapped"
            );
            self.collect(machines)?;
        }
        anyhow::bail!("paired instruction budget exhausted")
    }
}

impl WonderSwanPairSnapshot {
    pub fn frame(&self) -> u64 {
        self.schedule.frame
    }

    pub fn checksum(&self) -> [u8; 32] {
        self.checksum
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn shared_media_bytes(&self) -> usize {
        self.shared_media_bytes
    }

    pub fn native_state(&self, endpoint: Endpoint) -> Result<Vec<u8>> {
        self.machines[endpoint.index()].encode_state()
    }
}

impl Emulator {
    pub fn clone_for_link_peer(&self) -> Self {
        let mut peer = self.clone();
        peer.link_identity = Arc::new(());
        peer
    }
}
