use anyhow::{Result, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use zeff_audio_discovery::huge::isolation::IsolatedRom;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_gb_core::{
    emulator::Emulator,
    hardware::types::{CpuState, ImeState, hardware_mode::HardwareMode},
};

const FRAME_CYCLES: u64 = 70_224;
const STACK_START: u16 = 0xffc0;
const ENTRY_SP: u16 = 0xfff2;
const STACK_END: u16 = 0xfffe;
const MAX_EVENTS: usize = 1_000_000;

pub fn required_frames(loop_ticks: u32) -> Result<u32> {
    let frames = zeff_audio_discovery::huge::catalog::required_validation_frames(loop_ticks)
        .ok_or_else(|| anyhow::anyhow!("invalid control recurrence budget"))?;
    ensure!(
        frames <= zeff_audio_discovery::huge::catalog::MAX_VALIDATION_FRAMES,
        "control recurrence exceeds 1024-frame budget"
    );
    Ok(frames)
}

fn control_period(loop_ticks: u32) -> Result<u32> {
    zeff_audio_discovery::huge::catalog::control_period_frames(loop_ticks)
        .ok_or_else(|| anyhow::anyhow!("invalid control period"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ControlState {
    pc: u16,
    sp: u16,
    registers: [u8; 8],
    driver_ram: Vec<u8>,
    bootstrap_hram: [u8; 2],
    live_stack: Vec<u8>,
    ie: u8,
    interrupt_flags: u8,
    nr51: u8,
    timer_control: u8,
    ppu_registers: [u8; 4],
    ppu_dot: u64,
    frame_phase: u64,
}

impl ControlState {
    fn capture(emulator: &Emulator, ram_start: u16, ram_end: u16) -> Self {
        let memory = |start, end| (start..end).map(|a| emulator.cpu_peek8(a)).collect();
        Self {
            pc: emulator.cpu_pc(),
            sp: emulator.cpu_sp(),
            registers: [
                emulator.cpu_a(),
                emulator.cpu_f(),
                emulator.cpu_b(),
                emulator.cpu_c(),
                emulator.cpu_d(),
                emulator.cpu_e(),
                emulator.cpu_h(),
                emulator.cpu_l(),
            ],
            driver_ram: memory(ram_start, ram_end),
            bootstrap_hram: [emulator.cpu_peek8(0xff80), emulator.cpu_peek8(0xff81)],
            live_stack: memory(ENTRY_SP, STACK_END),
            ie: emulator.ie_reg(),
            interrupt_flags: emulator.if_reg(),
            nr51: emulator.cpu_peek8(0xff25),
            timer_control: emulator.timer_tac(),
            ppu_registers: [
                emulator.ppu_lcdc(),
                emulator.ppu_stat(),
                emulator.ppu_ly(),
                emulator.ppu_lyc(),
            ],
            ppu_dot: emulator.ppu_cycles(),
            frame_phase: emulator.cpu_cycles() % FRAME_CYCLES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Access {
    cycle: u64,
    write: bool,
    address: u16,
    value: u8,
    mapped_address: Option<u32>,
}

#[derive(Serialize)]
struct Boundary {
    completed_updates: u32,
    cycle: u64,
    state: ControlState,
}

pub struct RecurrenceAudit {
    ram_start: u16,
    ram_end: u16,
    update: u16,
    warmup: u32,
    period: u32,
    update_count: u32,
    previous_update: Option<u64>,
    boundaries: Vec<Boundary>,
    first_period: Vec<Access>,
    compared_events: usize,
    written_stack: [bool; (STACK_END - STACK_START) as usize],
    last_cycle: u64,
    nr51: u8,
    sound_writes: usize,
}

impl RecurrenceAudit {
    pub fn new(plan: &IsolatedRom, update: u16, loop_ticks: u32) -> Result<Self> {
        required_frames(loop_ticks)?;
        ensure!(
            plan.ram_end.checked_sub(plan.ram_start) == Some(100),
            "driver RAM size differs"
        );
        ensure!(
            (plan.driver_start..plan.driver_end).contains(&update),
            "update escapes driver"
        );
        Ok(Self {
            ram_start: plan.ram_start,
            ram_end: plan.ram_end,
            update,
            warmup: loop_ticks,
            period: control_period(loop_ticks)?,
            update_count: 0,
            previous_update: None,
            boundaries: Vec::new(),
            first_period: Vec::new(),
            compared_events: 0,
            written_stack: [false; (STACK_END - STACK_START) as usize],
            last_cycle: 0,
            nr51: 0,
            sound_writes: 0,
        })
    }

    pub fn on_update(&mut self, emulator: &Emulator) -> Result<()> {
        ensure!(
            emulator.hardware_mode() == HardwareMode::DMG,
            "recurrence requires DMG"
        );
        ensure!(
            emulator.cpu_pc() == self.update && emulator.cpu_sp() == ENTRY_SP,
            "update CPU/stack entry differs"
        );
        ensure!(
            emulator.cpu_running() == CpuState::Running && emulator.cpu_ime() == ImeState::Disabled,
            "update CPU control differs"
        );
        ensure!(
            emulator.ie_reg() == 1 && emulator.if_reg() & 1 == 0,
            "update interrupt state differs"
        );
        ensure!(
            emulator.timer_tac() & 4 == 0 && emulator.ppu_lcdc() & 0x80 != 0,
            "scheduler requires disabled timer and enabled LCD"
        );
        let cycle = emulator.cpu_cycles();
        if let Some(previous) = self.previous_update {
            ensure!(
                cycle.checked_sub(previous) == Some(FRAME_CYCLES),
                "update cadence differs"
            );
        }
        self.previous_update = Some(cycle);
        if self.update_count >= self.warmup {
            let since_warmup = self.update_count - self.warmup;
            if since_warmup <= self.period * 2 && since_warmup.is_multiple_of(self.period) {
                self.boundary(
                    cycle,
                    ControlState::capture(emulator, self.ram_start, self.ram_end),
                )?;
            }
        }
        self.update_count = self
            .update_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("update count overflow"))?;
        Ok(())
    }

    fn boundary(&mut self, cycle: u64, state: ControlState) -> Result<()> {
        ensure!(self.boundaries.len() < 3, "extra recurrence boundary");
        if let Some(previous) = self.boundaries.last() {
            ensure!(
                cycle.checked_sub(previous.cycle) == Some(u64::from(self.period) * FRAME_CYCLES),
                "recurrence cycle interval differs"
            );
            ensure!(
                state == previous.state,
                "complete driver/control state did not recur"
            );
            ensure!(state.nr51 == self.nr51, "unexplained NR51 latch change");
        }
        if self.boundaries.len() == 2 {
            ensure!(
                self.compared_events == self.first_period.len(),
                "repeated access count differs"
            );
        }
        self.nr51 = state.nr51;
        self.boundaries.push(Boundary {
            completed_updates: self.update_count,
            cycle,
            state,
        });
        self.written_stack.fill(false);
        self.last_cycle = 0;
        Ok(())
    }

    pub fn observe(&mut self, event: BusAccessEvent) -> Result<()> {
        if !matches!(self.boundaries.len(), 1 | 2) {
            return Ok(());
        }
        ensure!(
            event.space() == TraceWriteKind::Memory && event.width() == TraceWriteWidth::Byte,
            "non-byte memory access in recurrence"
        );
        let at = event
            .at()
            .ok_or_else(|| anyhow::anyhow!("missing access timestamp"))?
            .get();
        let start = self.boundaries.last().unwrap().cycle;
        let cycle = at
            .checked_sub(start)
            .ok_or_else(|| anyhow::anyhow!("access precedes boundary"))?;
        ensure!(
            cycle >= self.last_cycle && cycle <= u64::from(self.period) * FRAME_CYCLES,
            "access timestamp outside control period"
        );
        self.last_cycle = cycle;
        let address = u16::try_from(event.addr())?;
        let (write, value, mapped_address) = match event {
            BusAccessEvent::Read {
                value, mapped_addr, ..
            } => (false, value, mapped_addr),
            BusAccessEvent::Write {
                written_value,
                mapped_addr,
                ..
            } => (true, written_value, mapped_addr),
        };
        let value = u8::try_from(value)?;
        self.check_dependency(address, write, value)?;
        let access = Access {
            cycle,
            write,
            address,
            value,
            mapped_address,
        };
        if self.boundaries.len() == 1 {
            ensure!(
                self.first_period.len() < MAX_EVENTS,
                "recurrence access budget exhausted"
            );
            if write
                && ((0xff10..=0xff25).contains(&address) || (0xff30..=0xff3f).contains(&address))
            {
                self.sound_writes += 1;
            }
            self.first_period.push(access);
        } else {
            ensure!(
                self.first_period.get(self.compared_events) == Some(&access),
                "CPU access differs in repeated period at event {}",
                self.compared_events
            );
            self.compared_events += 1;
        }
        Ok(())
    }

    fn check_dependency(&mut self, address: u16, write: bool, value: u8) -> Result<()> {
        match address {
            0x0000..=0x7fff => ensure!(!write, "mutable ROM dependency"),
            a if (self.ram_start..self.ram_end).contains(&a) => {}
            0xff80..=0xff81 => {}
            STACK_START..=0xfffd => {
                let initialized = &mut self.written_stack[usize::from(address - STACK_START)];
                if write {
                    *initialized = true;
                } else {
                    ensure!(
                        address >= ENTRY_SP || *initialized,
                        "dead stack read before period write at {address:#06x}"
                    );
                }
            }
            0xff25 => {
                if write {
                    self.nr51 = value;
                } else {
                    ensure!(
                        value == self.nr51,
                        "NR51 read differs from deterministic latch"
                    );
                }
            }
            0xff10..=0xff24 | 0xff30..=0xff3f => {
                ensure!(write, "APU phase-dependent read at {address:#06x}")
            }
            _ => bail!("unsupported recurring dependency at {address:#06x}"),
        }
        Ok(())
    }

    pub fn report(&self) -> Result<Value> {
        ensure!(
            self.boundaries.len() == 3 && !self.first_period.is_empty(),
            "incomplete control recurrence"
        );
        ensure!(
            self.compared_events == self.first_period.len(),
            "incomplete repeated access period"
        );
        Ok(json!({
            "schema": "zeff-huge-control-recurrence/1",
            "passed": true,
            "warmup_updates": self.warmup,
            "period_updates": self.period,
            "period_cycles": u64::from(self.period) * FRAME_CYCLES,
            "boundaries": self.boundaries,
            "cpu_ime": "disabled",
            "cpu_state": "running",
            "period_accesses": self.first_period.len(),
            "period_accesses_sha256": zeff_firmware::sha256_hex(&serde_json::to_vec(&self.first_period)?),
            "period_sound_writes": self.sound_writes,
            "dead_stack_contract": "FFC0..FFF1 must be written within each period before any read",
            "scope": "Two identical periods of complete driver/control state and CPU accesses under the validated DMG bootstrap; APU oscillator, envelope, filter and sample phase are excluded and PCM looping is not established."
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::time::MasterTicks;

    fn audit() -> RecurrenceAudit {
        RecurrenceAudit {
            ram_start: 0xc000,
            ram_end: 0xc064,
            update: 0x600,
            warmup: 128,
            period: 256,
            update_count: 128,
            previous_update: None,
            boundaries: Vec::new(),
            first_period: Vec::new(),
            compared_events: 0,
            written_stack: [false; (STACK_END - STACK_START) as usize],
            last_cycle: 0,
            nr51: 0,
            sound_writes: 0,
        }
    }

    fn state() -> ControlState {
        ControlState {
            pc: 0x600,
            sp: ENTRY_SP,
            registers: [0; 8],
            driver_ram: vec![0; 100],
            bootstrap_hram: [1, 128],
            live_stack: vec![0; 12],
            ie: 1,
            interrupt_flags: 0,
            nr51: 255,
            timer_control: 0,
            ppu_registers: [0x91, 1, 144, 0],
            ppu_dot: 104,
            frame_phase: 65768,
        }
    }

    fn read(cycle: u64, address: u16, value: u8) -> BusAccessEvent {
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(cycle)),
            space: TraceWriteKind::Memory,
            addr: u32::from(address),
            value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    }

    fn write(cycle: u64, address: u16, value: u8) -> BusAccessEvent {
        BusAccessEvent::Write {
            at: Some(MasterTicks::new(cycle)),
            space: TraceWriteKind::Memory,
            addr: u32::from(address),
            old_value: 0,
            written_value: u32::from(value),
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    }

    fn begin() -> RecurrenceAudit {
        let mut audit = audit();
        audit.boundary(0, state()).unwrap();
        audit
    }

    #[test]
    fn budgets_include_counter_and_bootstrap_wrap_and_warmup() {
        assert_eq!(control_period(128).unwrap(), 256);
        assert_eq!(control_period(192).unwrap(), 768);
        assert_eq!(required_frames(128).unwrap(), 641);
        assert_eq!(required_frames(256).unwrap(), 769);
        assert!(required_frames(0).is_err());
        assert!(required_frames(192).is_err());
        assert!(required_frames(u32::MAX).is_err());
    }

    #[test]
    fn rejects_each_changed_driver_and_control_component() {
        let original = state();
        let mut variants = Vec::new();
        for i in 0..100 {
            let mut changed = original.clone();
            changed.driver_ram[i] ^= 1;
            variants.push(changed);
        }
        for i in 0..8 {
            let mut changed = original.clone();
            changed.registers[i] ^= 1;
            variants.push(changed);
        }
        for i in 0..12 {
            let mut changed = original.clone();
            changed.live_stack[i] ^= 1;
            variants.push(changed);
        }
        for i in 0..2 {
            let mut changed = original.clone();
            changed.bootstrap_hram[i] ^= 1;
            variants.push(changed);
        }
        for change in [
            |s: &mut ControlState| s.sp ^= 1,
            |s: &mut ControlState| s.pc ^= 1,
            |s: &mut ControlState| s.ie ^= 1,
            |s: &mut ControlState| s.interrupt_flags ^= 1,
            |s: &mut ControlState| s.nr51 ^= 1,
            |s: &mut ControlState| s.timer_control ^= 1,
            |s: &mut ControlState| s.ppu_dot ^= 1,
            |s: &mut ControlState| s.ppu_registers[0] ^= 1,
            |s: &mut ControlState| s.frame_phase ^= 1,
        ] {
            let mut changed = original.clone();
            change(&mut changed);
            variants.push(changed);
        }
        for changed in variants {
            assert!(begin().boundary(256 * FRAME_CYCLES, changed).is_err());
        }
        assert!(begin().boundary(128 * FRAME_CYCLES, original).is_err());
    }

    #[test]
    fn dead_stack_requires_fresh_writes_in_each_period() {
        let mut audit = begin();
        assert!(audit.observe(read(4, 0xfff0, 1)).is_err());
        audit.observe(write(8, 0xfff0, 1)).unwrap();
        audit.observe(read(12, 0xfff0, 1)).unwrap();
        audit.boundary(256 * FRAME_CYCLES, state()).unwrap();
        assert!(
            audit
                .observe(read(256 * FRAME_CYCLES + 4, 0xfff0, 1))
                .is_err()
        );
    }

    #[test]
    fn rejects_apu_feedback_and_scheduler_changes() {
        for addr in [0xff10, 0xff26, 0xff30, 0xff04, 0xff0f, 0xffff, 0xc064] {
            assert!(begin().observe(read(4, addr, 0)).is_err());
        }
        for addr in [
            0xff26, 0xff04, 0xff07, 0xff0f, 0xff40, 0xff46, 0xffff, 0x200,
        ] {
            assert!(begin().observe(write(4, addr, 0)).is_err());
        }
        assert!(begin().observe(read(4, 0xff25, 0)).is_err());
        let mut audit = begin();
        audit.observe(write(4, 0xff25, 0xbb)).unwrap();
        audit.observe(read(8, 0xff25, 0xbb)).unwrap();
        assert!(audit.boundary(256 * FRAME_CYCLES, state()).is_err());
    }

    #[test]
    fn repeats_cpu_accesses_without_comparing_unread_apu_state() {
        let period_cycles = 256 * FRAME_CYCLES;
        let mut audit = begin();
        audit.observe(read(4, 0xff25, 255)).unwrap();
        audit.observe(write(8, 0xff12, 0xf0)).unwrap();
        audit.boundary(period_cycles, state()).unwrap();
        audit.observe(read(period_cycles + 4, 0xff25, 255)).unwrap();
        let mut changed_apu = write(period_cycles + 8, 0xff12, 0xf0);
        if let BusAccessEvent::Write {
            old_value,
            new_value,
            ..
        } = &mut changed_apu
        {
            *old_value = 1;
            *new_value = 2;
        }
        audit.observe(changed_apu).unwrap();
        audit.boundary(period_cycles * 2, state()).unwrap();
        let report = audit.report().unwrap();
        assert_eq!(report["period_accesses"], 2);
        assert_eq!(report["period_sound_writes"], 1);
    }

    #[test]
    fn rejects_access_value_order_timing_and_incomplete_periods() {
        let period_cycles = 256 * FRAME_CYCLES;
        for bad in [
            read(period_cycles + 4, 0xc000, 2),
            read(period_cycles + 8, 0xc000, 1),
            read(period_cycles + 4, 0xc001, 1),
            write(period_cycles + 4, 0xc000, 1),
        ] {
            let mut audit = begin();
            audit.observe(read(4, 0xc000, 1)).unwrap();
            audit.boundary(period_cycles, state()).unwrap();
            assert!(audit.observe(bad).is_err());
        }
        let mut audit = begin();
        audit.observe(read(8, 0xc000, 1)).unwrap();
        assert!(audit.observe(read(4, 0xc000, 1)).is_err());
        assert!(audit.report().is_err());
        audit.boundary(period_cycles, state()).unwrap();
        assert!(audit.boundary(period_cycles * 2, state()).is_err());
    }
}
