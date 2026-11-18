use super::*;
use crate::hardware::bus::{Bus, ownership::Requester};
use crate::hardware::sound::{
    BankAccess, CandidateBus, Opn2Writes, PsgWrites, SoundMemory, WorkWrites,
};
use zeff_z80::{BusState, Cpu, FetchedInstruction};

pub(super) trait CasePolicy: Clone + std::fmt::Debug {
    const GATE_RESET_REQUEST: bool;
    fn prefix_t(&self) -> u64;
    fn expected_target(&self) -> (u32, bool, bool);
    fn after_publication(&self, bus: &mut Bus, any_bank_access: bool);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Segment {
    cpu: Cpu,
    memory: SoundMemory,
    instruction: FetchedInstruction,
    state: BusState,
    writes: WorkWrites,
    psg: PsgWrites,
    opn2: Opn2Writes,
    registers: String,
    bank: Option<BankAccess>,
    any: bool,
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) target: bool,
    origin: Option<zeff_emu_common::audio_trace::MdTraceOrigin>,
}

#[derive(Clone)]
pub(super) struct Oracle<C> {
    pub bus: Bus,
    pub cpu: Cpu,
    pub state: BusState,
    pub pending: Option<Segment>,
    pub(super) checkpoint: Option<Segment>,
    pub lock: Option<(u64, u64)>,
    pub deferrals: usize,
    pub raw_t: u64,
    pub target: Option<(u64, u64)>,
    pub target_publications: usize,
    case: C,
}

impl<C: CasePolicy> Oracle<C> {
    pub fn new(machine: &Emulator, case: &C) -> Self {
        Self {
            bus: machine.bus.clone(),
            cpu: machine.sound_cpu.clone(),
            state: Default::default(),
            pending: None,
            checkpoint: None,
            lock: None,
            deferrals: 0,
            raw_t: 0,
            target: None,
            target_publications: 0,
            case: case.clone(),
        }
    }

    fn prepare(&mut self, at: u64) {
        let mut memory = self.bus.sound.clone();
        let mut cpu = self.cpu.clone();
        let mut state = self.state;
        let qualified = state.next_memory_cycle();
        let mut bus = CandidateBus::new(&mut memory, &self.bus.cartridge);
        let instruction = cpu
            .step_bus_segment(&mut bus, &mut state)
            .unwrap()
            .instruction;
        assert_eq!(bus.fault.get(), None);
        assert_eq!(cpu.trap(), None);
        let any = bus.bank_access.get().is_some();
        let bank = if qualified == Some(instruction.cycles) && bus.accesses.get() == 1 {
            bus.bank_access.get()
        } else {
            None
        };
        let target = self.raw_t == self.case.prefix_t();
        if target {
            assert_eq!(
                (instruction.cycles, bank.is_some(), any),
                self.case.expected_target()
            );
        }
        let writes = bus.writes;
        let psg = bus.psg_writes;
        let opn2 = bus.opn2_writes;
        let registers = format!("{:?}", bus.register_writes);
        self.raw_t += u64::from(instruction.cycles);
        let start = at / 15 * 15;
        let end = start + u64::from(instruction.cycles) * 15;
        let origin = self.bus.audio.trace.is_enabled().then(|| {
            crate::hardware::audio::trace::sound_origin(
                self.cpu.regs().pc,
                self.bus.sound.bank,
                &self.bus.cartridge,
            )
        });
        self.pending = Some(Segment {
            cpu,
            memory,
            instruction,
            state,
            writes,
            psg,
            opn2,
            registers,
            bank,
            any,
            start,
            end,
            target,
            origin,
        });
    }

    pub(super) fn reserve(&mut self, occupied: Option<(u64, u64, bool)>) {
        let Some((a, e, exclusive)) = occupied else {
            return;
        };
        let p = self.pending.as_mut().unwrap();
        let external = p.bank.is_some() || p.any;
        if !external || (!exclusive && p.bank.is_none()) || p.start >= e || a >= p.end {
            return;
        }
        if exclusive && self.checkpoint.is_none() {
            self.checkpoint = Some(p.clone());
            self.deferrals += 1;
        }
        let wait = (e - p.start).div_ceil(15);
        p.start += wait * 15;
        p.end += wait * 15;
        assert!(p.cpu.extend_bus_segment(&mut p.instruction, wait as u32));
    }

    pub fn advance_sound(&mut self, begin: u64, end: u64, occupied: Option<(u64, u64, bool)>) {
        let mut cursor = begin;
        while cursor < end {
            if C::GATE_RESET_REQUEST
                && self.pending.is_none()
                && (self.bus.sound.reset_asserted || self.bus.sound.requested)
            {
                self.bus.sound.complete_boundary();
                break;
            }
            if self.pending.is_none() {
                self.prepare(cursor);
            }
            self.reserve(occupied);
            let p = self.pending.as_ref().unwrap();
            if p.target {
                self.target = Some((p.start, p.end));
            }
            if p.end > end {
                break;
            }
            let p = self.pending.take().unwrap();
            if p.target {
                self.target_publications += 1;
            }
            self.bus.audio.request_advance_to(p.end);
            let previous_origin = self.bus.audio.trace_origin;
            if self.bus.audio.trace.is_enabled() {
                self.bus.audio.trace_origin = p.origin;
            }
            p.psg.publish(&mut self.bus.audio, p.end);
            p.opn2.publish(&mut self.bus.audio, p.end);
            self.bus.audio.trace_origin = previous_origin;
            self.cpu = p.cpu;
            self.state = p.state;
            self.bus.sound.ram = p.memory.ram;
            self.bus.sound.bank = p.memory.bank;
            p.writes.publish(&mut self.bus.ram);
            self.case.after_publication(&mut self.bus, p.any);
            self.bus.sound.complete_boundary();
            cursor = p.end;
        }
        self.bus.audio.request_advance_to(end);
    }

    pub fn advance(&mut self, end: u64, occupied: Option<(u64, u64, bool)>) {
        let begin = self.bus.now.get();
        self.advance_sound(begin, end, occupied);
        self.bus.advance_machine(end - begin).unwrap();
        self.bus.cpu_clock.0 += (end - begin) / 7;
    }

    pub fn future(&mut self, end: u64) {
        let clock = self.bus.cpu_clock;
        self.advance(end, None);
        self.bus.cpu_clock = clock;
    }

    pub fn ordinary(&mut self) -> u32 {
        let begin = self.bus.now.get();
        let release = self
            .pending
            .as_ref()
            .filter(|p| p.bank.is_some())
            .map_or(begin, |p| p.end.max(begin));
        let waits = (release - begin).div_ceil(7) as u32;
        let transfer = begin + u64::from(waits) * 7;
        if waits != 0 {
            self.advance(transfer, Some((release, transfer + 28, false)));
        }
        self.advance(transfer + 28, Some((transfer, transfer + 28, false)));
        waits
    }

    pub fn read(&mut self) -> u32 {
        let begin = self.bus.now.get();
        let release = self
            .pending
            .as_ref()
            .filter(|p| p.bank.is_some() || p.any)
            .map_or(begin, |p| p.end.max(begin));
        let waits = (release - begin).div_ceil(7) as u32;
        let a = begin + u64::from(waits) * 7;
        self.lock = Some((a, a + 70));
        if waits != 0 {
            self.advance(a, Some((a, a + 70, true)));
        }
        self.advance(a + 28, Some((a, a + 70, true)));
        waits
    }

    pub fn held(&mut self, ticks: u64) {
        let (a, e) = self.lock.unwrap();
        self.advance(self.bus.now.get() + ticks, Some((a, e, true)));
    }

    pub fn check(&self, actual: &ScheduledBus<'_>) {
        let mut cpu = actual.cpu.clone();
        let mut schedule = actual.schedule.clone();
        schedule.materialize_epoch(
            &mut cpu,
            &actual.bus.sound,
            &mut crate::sound_trace::NoSoundCapture,
        );
        assert_eq!(cpu, self.cpu, "{:?}", self.case);
        assert_eq!(schedule.state, self.state);
        assert_eq!(schedule.fault, None);
        assert_eq!(actual.bus.ram, self.bus.ram);
        assert_eq!(actual.bus.sound, self.bus.sound);
        assert_eq!(actual.bus.cartridge, self.bus.cartridge);
        assert_eq!(actual.bus.audio, self.bus.audio);
        assert_eq!(actual.bus.vdp, self.bus.vdp);
        assert_eq!(actual.bus.video, self.bus.video);
        assert_eq!(actual.bus.now, self.bus.now);
        assert_eq!(actual.bus.cpu_clock, self.bus.cpu_clock);
        match (&schedule.pending, &self.pending) {
            (Some(p), Some(q)) => Self::check_segment(p, q),
            (None, None) => {}
            _ => panic!("pending presence: {:?}", self.case),
        }
        match (&schedule.rmw_checkpoint, &self.checkpoint) {
            (Some(p), Some(q)) => Self::check_segment(p, q),
            (None, None) => {}
            _ => panic!("checkpoint presence: {:?}", self.case),
        }
        let reservation = actual.bus.rmw.as_ref().and_then(|p| p.reservation.as_ref());
        assert_eq!(reservation.map(|p| (p.ticks.start, p.ticks.end)), self.lock);
        if let Some(p) = reservation {
            assert_eq!(p.requester, Requester::MAIN_RMW);
        }
    }

    fn check_segment(p: &crate::hardware::sound::schedule::PendingSound, q: &Segment) {
        assert_eq!((p.start, p.deadline), (q.start, q.end));
        assert_eq!(p.cpu, q.cpu);
        assert_eq!(p.memory, q.memory);
        assert_eq!(p.instruction, q.instruction);
        assert_eq!(p.state, q.state);
        assert_eq!(p.writes, q.writes);
        assert_eq!(p.psg_writes, q.psg);
        assert_eq!(p.opn2_writes, q.opn2);
        assert_eq!(format!("{:?}", p.register_writes), q.registers);
        assert_eq!(p.bank_access, q.bank);
        assert_eq!(p.any_bank_access, q.any);
        assert_eq!(p.trace_origin, q.origin);
    }
}
