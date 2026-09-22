use std::sync::atomic::{AtomicBool, Ordering};

use super::control::{self, Control};
use anyhow::{Result, ensure};
use serde_json::Value;
use zeff_audio_discovery::{
    huge::{catalog::HugeSong, gbs::ExperimentalGbs},
    tracker::FileSpan,
};
use zeff_emu_common::time::MasterTicks;
use zeff_gb_core::{
    emulator::Emulator,
    hardware::{
        bus::CpuAccessTraceEvent,
        cpu::GbCpuBus,
        types::{CpuState, ImeState},
    },
    save_state::{SaveState, decode_on_thread},
};

use super::native::{Call, normalized_ram, sound_address};

const STACK: u16 = 0xfff0;
const SENTINEL: u16 = 0xffb0;
const PERIOD: u64 = 70_224;

pub struct Host {
    state: SaveState,
    init: u16,
    play: u16,
    driver: u16,
    update: u16,
    ram: u16,
    delta: u16,
    readable: Vec<FileSpan>,
    executable: Vec<FileSpan>,
    initialized: Vec<bool>,
    pub writes: Vec<(u64, u16, u8)>,
    pub access_count: usize,
    loop_ticks: u32,
    access_hashes: Vec<String>,
    pub recurrence: Value,
}

impl Host {
    pub fn source(bytes: &[u8], song: &HugeSong) -> Result<Self> {
        let init = reference_init(
            song.bound.song.descriptor.offset as u16,
            song.bound.evidence.init_address,
        );
        let play = [
            0xcd,
            song.bound.evidence.update_address as u8,
            (song.bound.evidence.update_address >> 8) as u8,
            0xc9,
        ];
        let wrappers = vec![span(0xff80, init.len()), span(0xffa8, play.len())];
        let mut host = Self::new(bytes.to_vec(), song, 0, wrappers, 0xff80, 0xffa8)?;
        for (address, code) in [(0xff80, init.as_slice()), (0xffa8, play.as_slice())] {
            for (offset, value) in code.iter().enumerate() {
                host.state.bus.write_byte(address + offset as u16, *value);
            }
        }
        Ok(host)
    }

    pub fn artifact(artifact: &ExperimentalGbs, song: &HugeSong, fill: u8) -> Result<Self> {
        let bytes = &artifact.bytes;
        ensure!(
            bytes.len() == 0x7c70 && &bytes[..6] == b"GBS\x01\x01\x01",
            "unexpected GBS framing"
        );
        ensure!(
            word(bytes, 6) == 0x400 && word(bytes, 12) == STACK && bytes[14..16] == [0, 0],
            "unsupported GBS host contract"
        );
        ensure!(
            word(bytes, 8) == artifact.init_wrapper.offset as u16
                && word(bytes, 10) == artifact.play_wrapper.offset as u16,
            "GBS entry changed"
        );
        let mut rom = vec![fill; 0x8000];
        rom[0x400..].copy_from_slice(&bytes[0x70..]);
        rom[..0x400].fill(0);
        if fill != 0 {
            for address in 0x400..0x8000 {
                if !artifact
                    .copied_target_spans
                    .iter()
                    .chain([&artifact.init_wrapper, &artifact.play_wrapper])
                    .any(|s| contains(s, address))
                {
                    rom[address as usize] = fill;
                }
            }
        }
        Self::new(
            rom,
            song,
            artifact.translation_delta,
            vec![artifact.init_wrapper, artifact.play_wrapper],
            word(bytes, 8),
            word(bytes, 10),
        )
    }

    fn new(
        rom: Vec<u8>,
        song: &HugeSong,
        delta: u16,
        wrappers: Vec<FileSpan>,
        init: u16,
        play: u16,
    ) -> Result<Self> {
        let emulator = Emulator::new(&rom, 48_000)?;
        let mut state = decode_on_thread(emulator.encode_state()?)?;
        state.bus.cartridge.restore_rom_bytes(rom);
        state.bus.trace_cpu_accesses = true;
        state.bus.set_apu_sample_generation_enabled(true);
        state.cpu.pc = SENTINEL;
        state.cpu.sp = STACK;
        state.cpu.regs = zeff_gb_core::hardware::cpu::Registers {
            a: 0,
            f: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
        };
        state.bus.wram.fill(0);
        state.bus.hram.fill(0);
        state.cpu.ime = ImeState::Disabled;
        state.cpu.running = CpuState::Running;
        ensure!(state.cpu.cycles == 0, "host must start at cycle zero");
        let driver = song
            .bound
            .evidence
            .init_address
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("driver relocation overflow"))?;
        let update = song
            .bound
            .evidence
            .update_address
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("update relocation overflow"))?;
        let driver_span = span(driver, song.bound.evidence.driver.byte_len as usize);
        let mut readable: Vec<_> = song
            .bound
            .song
            .spans
            .iter()
            .map(|s| FileSpan {
                offset: s.offset + u32::from(delta),
                byte_len: s.byte_len,
            })
            .collect();
        readable.push(driver_span);
        readable.extend(&wrappers);
        let mut executable = wrappers;
        executable.push(driver_span);
        Ok(Self {
            state,
            init,
            play,
            driver,
            update,
            ram: song.bound.evidence.ram_address,
            delta,
            readable,
            executable,
            initialized: vec![false; 0x10000],
            writes: Vec::new(),
            access_count: 0,
            loop_ticks: song.bound.song.loop_ticks,
            access_hashes: Vec::new(),
            recurrence: Value::Null,
        })
    }

    pub fn run(&mut self, native: &[Call], cancel: &AtomicBool) -> Result<Vec<f32>> {
        let period = zeff_audio_discovery::huge::catalog::control_period_frames(self.loop_ticks)
            .ok_or_else(|| anyhow::anyhow!("invalid GBS loop"))? as usize;
        let boundaries = [
            self.loop_ticks as usize,
            self.loop_ticks as usize + period,
            self.loop_ticks as usize + period * 2,
        ];
        let mut states = Vec::new();
        for (index, expected) in native.iter().enumerate() {
            self.idle_until(index as u64 * PERIOD, cancel)?;
            self.call(
                if index == 0 { self.init } else { self.play },
                if index == 0 { self.driver } else { self.update },
                expected,
                cancel,
            )?;
            if boundaries.contains(&index) {
                states.push((index, Control::capture(&self.state, self.ram)));
            }
            ensure!(
                self.state.cpu.cycles < (index as u64 + 1) * PERIOD,
                "GBS call overruns cadence"
            );
        }
        self.idle_until(native.len() as u64 * PERIOD, cancel)?;
        self.recurrence = control::validate(&states, &self.access_hashes, self.loop_ticks)?;
        Ok(self.state.bus.apu_drain_samples())
    }

    fn call(
        &mut self,
        entry: u16,
        driver: u16,
        expected: &Call,
        cancel: &AtomicBool,
    ) -> Result<()> {
        ensure!(
            self.state.cpu.pc == SENTINEL
                && self.state.cpu.sp == STACK
                && self.state.cpu.ime == ImeState::Disabled,
            "GBS caller state changed"
        );
        self.initialized[0xffc0..usize::from(STACK)].fill(false);
        self.state.cpu.sp -= 2;
        for (offset, value) in SENTINEL.to_le_bytes().iter().enumerate() {
            let address = self.state.cpu.sp + offset as u16;
            self.state.bus.write_byte(address, *value);
            self.initialized[usize::from(address)] = true;
        }
        self.state.cpu.pc = entry;
        let call_start = self.state.cpu.cycles;
        let mut access_bytes = Vec::new();
        let mut active = None;
        let mut observed = None;
        let mut driver_writes = Vec::new();
        for _ in 0..20_000 {
            ensure!(!cancel.load(Ordering::Relaxed), "GBS host cancelled");
            let pc = self.state.cpu.pc;
            ensure!(
                self.executable.iter().any(|s| contains(s, u32::from(pc))),
                "GBS executes outside driver/wrapper at {pc:04x}"
            );
            if pc == driver {
                ensure!(
                    active.is_none() && observed.is_none(),
                    "GBS calls driver more than once"
                );
                let sp = self.state.cpu.sp;
                let ret = u16::from_le_bytes([
                    self.state.bus.read_byte(sp),
                    self.state.bus.read_byte(sp + 1),
                ]);
                active = Some((self.state.cpu.cycles, sp + 2, ret));
            }
            let opcode = self.state.bus.read_byte(pc);
            self.state
                .bus
                .begin_cpu_access_trace_at(MasterTicks::new(self.state.cpu.cycles));
            self.state.cpu.step(&mut self.state.bus);
            let mut events = Vec::new();
            self.state
                .bus
                .drain_cpu_access_trace(|event| events.push(event));
            for event in events {
                let (at, address, value, write) = match event {
                    CpuAccessTraceEvent::Read {
                        at, addr, value, ..
                    } => (at, addr, value, 0u8),
                    CpuAccessTraceEvent::Write {
                        at,
                        addr,
                        written_value,
                        ..
                    } => (at, addr, written_value, 1u8),
                };
                let clock = at
                    .ok_or_else(|| anyhow::anyhow!("untimed GBS access"))?
                    .get();
                ensure!(clock >= call_start, "GBS access clock precedes call");
                access_bytes.extend((clock - call_start).to_le_bytes());
                access_bytes.extend(address.to_le_bytes());
                access_bytes.extend(value.to_le_bytes());
                access_bytes.push(write);
                if let CpuAccessTraceEvent::Write {
                    at: Some(at),
                    addr,
                    written_value,
                    ..
                } = event
                    && sound_address(addr as u16)
                {
                    self.writes
                        .push((at.get(), addr as u16, written_value as u8));
                    if let Some((base, _, _)) = active {
                        driver_writes.push((at.get() - base, addr as u16, written_value as u8));
                    }
                }
                self.audit(event, active.is_some())?;
            }
            if let Some((base, sp, ret)) = active
                && self.state.cpu.pc == ret
                && self.state.cpu.sp == sp
            {
                ensure!(
                    matches!(opcode, 0xc0 | 0xc8 | 0xd0 | 0xd8 | 0xc9),
                    "GBS driver did not return through RET"
                );
                observed = Some(Call {
                    cycles: self.state.cpu.cycles - base,
                    writes: std::mem::take(&mut driver_writes),
                    ram: normalized_ram(
                        (self.ram..self.ram + 100)
                            .map(|a| self.state.bus.read_byte(a))
                            .collect(),
                        self.delta,
                    )?,
                });
                active = None;
            }
            ensure!(
                self.state.cpu.running == CpuState::Running
                    && self.state.cpu.ime == ImeState::Disabled,
                "GBS halted or enabled interrupts"
            );
            if self.state.cpu.pc == SENTINEL {
                ensure!(
                    opcode == 0xc9 && self.state.cpu.sp == STACK && active.is_none(),
                    "GBS wrapper did not return through its caller stack"
                );
                ensure!(
                    observed.as_ref() == Some(expected),
                    "GBS driver call differs from original native execution"
                );
                self.access_hashes
                    .push(zeff_firmware::sha256_hex(&access_bytes));
                return Ok(());
            }
        }
        anyhow::bail!("GBS call did not return within its instruction budget")
    }

    fn audit(&mut self, event: CpuAccessTraceEvent, driver_active: bool) -> Result<()> {
        self.access_count += 1;
        ensure!(
            self.access_count <= 2_000_000,
            "GBS access budget exhausted"
        );
        let (address, write) = match event {
            CpuAccessTraceEvent::Read { addr, .. } => (u16::try_from(addr)?, false),
            CpuAccessTraceEvent::Write {
                addr,
                written_value,
                new_value,
                ..
            } => {
                ensure!(
                    written_value == new_value
                        || sound_address(addr as u16)
                        || (addr == 0xff0f && written_value == new_value & 0x1f),
                    "GBS write was blocked or transformed at {addr:04x}: {written_value:02x} -> {new_value:02x}"
                );
                (u16::try_from(addr)?, true)
            }
        };
        let stack = (0xffc0..STACK).contains(&address);
        let ram = (self.ram..self.ram + 100).contains(&address);
        if stack || ram {
            if write {
                self.initialized[usize::from(address)] = true;
            } else {
                ensure!(
                    self.initialized[usize::from(address)],
                    "GBS reads uninitialized RAM/stack {address:04x}"
                );
            }
        } else if write {
            ensure!(
                sound_address(address) || (!driver_active && matches!(address, 0xffff | 0xff0f)),
                "GBS writes outside its contract {address:04x}"
            );
        } else {
            ensure!(
                address == 0xff25
                    || self
                        .readable
                        .iter()
                        .any(|s| contains(s, u32::from(address))),
                "GBS reads outside its source {address:04x}"
            );
        }
        Ok(())
    }

    fn idle_until(&mut self, target: u64, cancel: &AtomicBool) -> Result<()> {
        ensure!(
            target >= self.state.cpu.cycles && (target - self.state.cpu.cycles).is_multiple_of(4),
            "GBS schedule missed its deadline"
        );
        while self.state.cpu.cycles < target {
            ensure!(!cancel.load(Ordering::Relaxed), "GBS idle cancelled");
            let timing = GbCpuBus::advance_cpu_t_cycles(&mut self.state.bus, 4);
            ensure!(
                timing.cpu_t_cycles == 4 && timing.master_ticks == 4,
                "GBS clock changed"
            );
            self.state.cpu.cycles += 4;
        }
        Ok(())
    }
}

fn reference_init(descriptor: u16, driver: u16) -> Vec<u8> {
    let mut code = vec![
        0xf3, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x26, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0xff, 0xe0,
        0x25, 0x3e, 0x77, 0xe0, 0x24, 0x21,
    ];
    code.extend(descriptor.to_le_bytes());
    code.push(0xcd);
    code.extend(driver.to_le_bytes());
    code.push(0xc9);
    code
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn span(address: u16, len: usize) -> FileSpan {
    FileSpan {
        offset: u32::from(address),
        byte_len: len as u32,
    }
}
fn contains(span: &FileSpan, address: u32) -> bool {
    (span.offset..span.offset + span.byte_len).contains(&address)
}
