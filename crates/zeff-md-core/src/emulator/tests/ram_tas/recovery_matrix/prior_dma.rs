use super::*;
use crate::hardware::bus::Bus;
use crate::hardware::vdp::VdpDmaSource;
use crate::{ControlCommand, VdpInterrupts};
use common::*;

pub(super) struct Outcome {
    pub sampled: u8,
    pub read_wait: u32,
    pub marks: Vec<(&'static str, u8)>,
}

struct Success<'a> {
    inner: Adapter<'a>,
    expected: Option<oracle::Oracle>,
    direct: Bus,
    sampled: u8,
    waits: u64,
    read_wait: u32,
    events: Vec<&'static str>,
    marks: Vec<(&'static str, u8)>,
}

fn release(bus: &Bus) -> Option<u64> {
    bus.vdp
        .external_dma_release(
            bus.region,
            VdpDmaSource::new(&bus.cartridge.bytes, &bus.ram),
        )
        .unwrap()
        .map(|t| t.get())
}

impl Success<'_> {
    fn mark(&mut self, phase: &'static str) {
        if let Some(expected) = &self.expected {
            expected.check(&self.inner.inner);
        } else {
            assert_eq!(self.inner.inner.bus.ram, self.direct.ram);
            assert_eq!(self.inner.inner.bus.vdp, self.direct.vdp);
            assert_eq!(self.inner.inner.bus.audio, self.direct.audio);
            assert_eq!(self.inner.inner.bus.video, self.direct.video);
            assert_eq!(self.inner.inner.bus.now, self.direct.now);
            assert_eq!(self.inner.inner.bus.cpu_clock, self.direct.cpu_clock);
            assert_eq!(self.inner.inner.bus.sound, self.direct.sound);
        }
        self.events.push(phase);
        if let Some(left) = self.inner.inner.bus.vdp.registers().registers()[19] {
            self.marks.push((phase, left));
        }
    }
    fn direct_advance(&mut self, ticks: u64) {
        self.direct.advance_machine(ticks).unwrap();
        self.direct.cpu_clock.0 += ticks / 7;
    }
}

impl ClockedBus for Success<'_> {
    type Error = BusError;
    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        let r = self.inner.begin_rmw(a, t);
        assert_eq!(r, Ok(BusResponse::Complete(())));
        self.mark("begin");
        r
    }
    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        let wait = if a.kind == AccessKind::Data {
            let source = self.expected.as_ref().map_or(&self.direct, |e| &e.bus);
            let now = source.now.get();
            let dma = release(source);
            let dma_wait = dma.map_or(0, |end| (end - now).div_ceil(7) as u32);
            if let Some(expected) = &mut self.expected {
                if let Some(end) = dma {
                    expected.vdp_advance(now + u64::from(dma_wait) * 7, end);
                }
                dma_wait + expected.read()
            } else {
                self.direct_advance(u64::from(dma_wait) * 7 + 28);
                dma_wait
            }
        } else if let Some(expected) = &mut self.expected {
            expected.ordinary()
        } else {
            self.direct_advance(28);
            0
        };
        let r = self.inner.read_cycle(a, t);
        assert_eq!(r.wait_clocks, wait);
        assert!(r.result.is_ok());
        self.waits += u64::from(wait);
        if a.kind == AccessKind::Data {
            let ram = self
                .expected
                .as_ref()
                .map_or(&self.direct.ram, |e| &e.bus.ram);
            self.sampled = ram[(a.logical_address & 0xffff) as usize];
            assert_eq!(r.result, Ok(u16::from_be_bytes([ram[0x0e], ram[0x0f]])));
            self.read_wait = wait;
            self.mark("read");
        } else {
            self.mark("prefetch");
        }
        r
    }
    fn idle(&mut self, t: CpuClocks, n: u32) {
        assert_eq!(n, 2);
        if let Some(expected) = &mut self.expected {
            expected.held(14);
        } else {
            self.direct_advance(14);
        }
        self.inner.idle(t, n);
        self.mark("idle");
    }
    fn write_cycle(&mut self, a: BusAccess, v: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        if let Some(expected) = &mut self.expected {
            expected.held(28);
        } else {
            self.direct_advance(28);
        }
        let r = self.inner.write_cycle(a, v, t);
        assert_eq!(r, CycleResult::ready(()));
        self.mark("write");
        r
    }
    fn end_rmw(&mut self, t: CpuClocks) {
        if let Some(expected) = &mut self.expected {
            expected.end();
        }
        self.inner.end_rmw(t);
        self.mark("end");
    }
}

pub(super) fn success(machine: &mut Emulator, expected: Option<oracle::Oracle>) -> Outcome {
    let initial_clock = machine.cpu.clock().0;
    let mut registers = machine.cpu.registers;
    let concurrent = machine.concurrent_sound;
    let mut bus = Success {
        direct: machine.bus.clone(),
        inner: Adapter {
            concurrent,
            inner: ScheduledBus::new(
                &mut machine.bus,
                &mut machine.sound_cpu,
                &mut machine.sound_schedule,
            ),
        },
        expected,
        sampled: 0,
        waits: 0,
        read_wait: 0,
        events: Vec::new(),
        marks: Vec::new(),
    };
    let report = machine.cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(report.opcode, 0x4ad0);
    assert_eq!(report.clocks, 14 + bus.waits);
    assert_eq!(
        bus.events,
        ["begin", "read", "idle", "write", "end", "prefetch"]
    );
    registers.pc += 2;
    registers.set_status_register(
        0x2710 | if bus.sampled == 0 { 4 } else { 0 } | if bus.sampled & 0x80 != 0 { 8 } else { 0 },
    );
    assert_eq!(
        machine.cpu,
        complete_cpu(registers, [0x4e71, 0x4e71], initial_clock + report.clocks)
    );
    let result = Outcome {
        sampled: bus.sampled,
        read_wait: bus.read_wait,
        marks: bus.marks.clone(),
    };
    let expected = bus.expected.take();
    drop(bus);
    if let Some(expected) = expected {
        future(machine, expected);
    }
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    result
}

pub(super) fn prior_rows() {
    let mut count = 0;
    for kind in [Kind::Immediate, Kind::Ldi] {
        for residue in 0..15 {
            for lane in 0..2 {
                for state in 0..3 {
                    let edge = match state {
                        0 => -60,
                        1 => -15,
                        _ => -30,
                    };
                    let case = Case {
                        kind,
                        residue,
                        lane,
                        edge,
                    };
                    let (mut machine, expected) =
                        seed_ram_mode(true, &case, state == 0, state == 2);
                    if state == 0 {
                        assert!(machine.sound_schedule.pending.is_none());
                    } else {
                        let pending = machine.sound_schedule.pending.as_ref().unwrap();
                        assert!(pending.bank_access.is_some());
                        assert!(pending.any_bank_access);
                        if state == 2 {
                            assert_eq!(pending.start, case.s() + 15);
                        }
                    }
                    let outcome = success(&mut machine, expected);
                    assert_eq!(outcome.sampled, 0xa6);
                    assert_eq!(outcome.read_wait == 0, state == 0);
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 180);
}

fn device(at: u64, count: u8, mode: u8, local: bool) -> Vdp {
    let origin = if local { at - 300 } else { at };
    let mut regs = *Vdp::bootstrap_mode5().registers();
    let mut irq = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    for (i, v) in [
        (1, 0x14),
        (12, 0),
        (15, if local { 1 } else { 2 }),
        (19, count),
        (20, 0),
        (21, 0),
        (22, if local { 0 } else { 0xc0 }),
        (23, mode),
    ] {
        let at = MasterTicks::new(regs.now().get() + 1);
        while irq
            .advance_pair(at, Region::NtscUs, &mut regs, false)
            .unwrap()
            .is_some()
        {}
        let write = regs.queue_register(i, v).unwrap();
        while irq
            .advance_pair(write.publish_at, Region::NtscUs, &mut regs, false)
            .unwrap()
            .is_some()
        {}
    }
    while irq
        .advance_pair(MasterTicks::new(origin), Region::NtscUs, &mut regs, false)
        .unwrap()
        .is_some()
    {}
    let mut vdp = Vdp::from_stable_display_state(irq, regs, Region::NtscUs).unwrap();
    if local {
        vdp.write_control(0x4000, Region::NtscUs).unwrap();
        vdp.write_control(0, Region::NtscUs).unwrap();
        vdp.write_data(0x1234, Region::NtscUs).unwrap();
        vdp.advance_to(MasterTicks::new(at), Region::NtscUs)
            .unwrap();
        vdp.resume_dma_from_known_phase(
            ControlCommand {
                address: 0x100,
                code: if mode == 0x80 { 0x21 } else { 0x30 },
            },
            (mode == 0x80).then_some(0xabcd),
            Region::NtscUs,
        )
        .unwrap();
    } else {
        vdp.write_control(0x4400, Region::NtscUs).unwrap();
        vdp.write_control(0x80, Region::NtscUs).unwrap();
    }
    vdp
}

fn external(machine: &mut Emulator, residue: u64) -> usize {
    machine.bus.ram[0x8000..0x8200].fill(0x44);
    for count in 17..=144_u8 {
        let vdp = device(machine.bus.now.get(), count, 0x7f, false);
        let end = vdp
            .external_dma_release(
                Region::NtscUs,
                VdpDmaSource::new(&machine.bus.cartridge.bytes, &machine.bus.ram),
            )
            .unwrap()
            .unwrap()
            .get();
        if end % 7 == residue {
            machine.bus.vdp = vdp;
            assert_eq!(release(&machine.bus).unwrap() % 7, residue);
            return count as usize;
        }
    }
    panic!("requested DMA residue has no real fixture");
}

pub(super) fn arm_external(machine: &mut Emulator, residue: u64) {
    external(machine, residue);
}

pub(super) fn dma_prior_rows() {
    let mut count = 0;
    for kind in [Kind::Immediate, Kind::Ldi] {
        for residue in 0..7 {
            for lane in 0..2 {
                let case = Case {
                    kind,
                    residue: 0,
                    lane,
                    edge: 0,
                };
                let (mut machine, mut expected) = seed_ram_mode(true, &case, false, false);
                let length = external(&mut machine, residue);
                let expected = expected.as_mut().unwrap();
                expected.bus.vdp = machine.bus.vdp.clone();
                expected.bus.ram = machine.bus.ram.clone();
                let result = success(&mut machine, Some(expected.clone()));
                assert_eq!(result.sampled, 0xa6);
                assert!(result.read_wait > 0);
                assert_eq!(
                    &machine.vdp().memory().vram()[0x400..0x400 + length * 2],
                    &vec![0x44; length * 2]
                );
                assert_eq!(machine.bus.ram[0x8000], 0x44);
                count += 1;
            }
        }
    }
    assert_eq!(count, 28);
}

pub(super) fn vdp_rows() {
    let mut count = 0;
    for residue in 0..7 {
        for lane in 0..2 {
            for concurrent in [false, true] {
                let case = Case {
                    kind: Kind::LocalOpn2,
                    residue: 0,
                    lane,
                    edge: -30,
                };
                let (mut machine, mut expected) = seed(concurrent, &case);
                let length = external(&mut machine, residue);
                if let Some(expected) = &mut expected {
                    expected.bus.vdp = machine.bus.vdp.clone();
                    expected.bus.ram = machine.bus.ram.clone();
                }
                let result = success(&mut machine, expected);
                assert!(result.read_wait > 0);
                if !concurrent {
                    assert!(release(&machine.bus).is_none());
                    let registers = machine.vdp().registers().registers();
                    assert_eq!(registers[1], Some(0x14));
                    assert_eq!(registers[12], Some(0));
                    assert_eq!((registers[19], registers[20]), (Some(0), Some(0)));
                    assert!(machine.vdp().fifo().len() <= 4);
                    let cpu = machine.cpu.clone();
                    let clock = machine.bus.cpu_clock;
                    let now = machine.bus.now.get();
                    advance(&mut machine, 180);
                    assert_eq!(machine.cpu, cpu);
                    assert_eq!(machine.bus.cpu_clock, clock);
                    assert_eq!(machine.bus.now.get(), now + 180);
                    assert_eq!(machine.vdp().now(), machine.bus.now);
                    assert!(machine.vdp().fifo().is_empty());
                }
                assert_eq!(
                    &machine.vdp().memory().vram()[0x400..0x400 + length * 2],
                    &vec![0x44; length * 2]
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 28);
    let mut local_count = 0;
    for mode in [0x80, 0xc0] {
        for phase in ["read", "idle", "write"] {
            let case = Case {
                kind: Kind::LocalPsg,
                residue: 0,
                lane: 0,
                edge: -30,
            };
            let (mut machine, mut expected) = seed(true, &case);
            machine.bus.vdp = device(case.a(), 16, mode, true);
            expected.as_mut().unwrap().bus.vdp = machine.bus.vdp.clone();
            let result = success(&mut machine, expected);
            let left = result.marks.iter().find(|(p, _)| *p == phase).unwrap().1;
            assert!(left > 0 && left <= 16);
            assert!(machine.vdp().registers().registers()[19].unwrap() < left);
            assert!(release(&machine.bus).is_none());
            assert_eq!(result.read_wait, 0);
            local_count += 1;
        }
    }
    assert_eq!(local_count, 6);
}
