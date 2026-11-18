#[path = "ram_tas/recovery_matrix.rs"]
mod recovery_matrix;

#[path = "ram_tas/sound_matrix.rs"]
mod sound_matrix;

#[path = "ram_tas/sound_oracle.rs"]
mod sound_oracle;

#[path = "ram_tas/isa.rs"]
mod isa;

#[path = "ram_tas/sound_first.rs"]
mod sound_first;

use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, BusResponse, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Read(BusAccess, u64, u32),
    Write(BusAccess, u16, u64, u32),
    Idle(u64, u32),
    Begin(BusAccess, u64),
    End(u64),
}

struct Observed<'a, B> {
    inner: &'a mut B,
    events: Vec<Event>,
}

impl<B: ClockedBus> ClockedBus for Observed<'_, B> {
    type Error = B::Error;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        let result = self.inner.read_cycle(access, start);
        self.events
            .push(Event::Read(access, start.0, result.wait_clocks));
        result
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        let result = self.inner.write_cycle(access, value, start);
        self.events
            .push(Event::Write(access, value, start.0, result.wait_clocks));
        result
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.events.push(Event::Idle(start.0, clocks));
        self.inner.idle(start, clocks);
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, Self::Error> {
        self.events.push(Event::Begin(access, start.0));
        self.inner.begin_rmw(access, start)
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        self.events.push(Event::End(end.0));
        self.inner.end_rmw(end);
    }
}

fn execute<B: ClockedBus>(cpu: &mut PrefetchCpu, inner: &mut B) -> Vec<Event>
where
    B::Error: std::fmt::Debug,
{
    let mut bus = Observed {
        inner,
        events: Vec::new(),
    };
    let step = cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(step.opcode, 0x4af9);
    assert_eq!(step.clocks, 22);
    bus.events
}

fn prefetch(address: u32, start: u64) -> Event {
    Event::Read(
        BusAccess {
            logical_address: address,
            bus_address: address,
            function_code: FunctionCode::SupervisorProgram,
            kind: AccessKind::Prefetch,
            lanes: ByteLanes::Both,
        },
        start,
        0,
    )
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_suppresses_setting_then_ordinary_bset_writes() {
    for concurrent in [false, true] {
        for parity in [0, 1] {
            for old in [0, 0x80] {
                let logical = 0xffff_000e + parity;
                let mut machine = super::machine(&[
                    0x4af9,
                    0xffff,
                    logical as u16,
                    0x08d0,
                    0x0000,
                    0x4e71,
                    0x4e71,
                ]);
                machine.concurrent_sound = concurrent;
                machine.cpu.registers.a[0] = logical;
                machine.cpu.registers.set_status_register(0x271f);
                machine.bus.ram[0x0e..0x10].copy_from_slice(&[0x35, 0x62]);
                machine.bus.ram[0x0e + parity as usize] = old;
                let ram = machine.bus.ram.clone();
                let mut expected = machine.cpu.registers;
                expected.pc = 0x106;
                expected.set_status_register(0x2710 | if old == 0 { 4 } else { 8 });
                let events = if concurrent {
                    let mut bus = CompactBus(ScheduledBus::new(
                        &mut machine.bus,
                        &mut machine.sound_cpu,
                        &mut machine.sound_schedule,
                    ));
                    execute(&mut machine.cpu, &mut bus)
                } else {
                    execute(&mut machine.cpu, &mut machine.bus)
                };
                let access = BusAccess {
                    logical_address: logical,
                    bus_address: 0xff000e,
                    function_code: FunctionCode::SupervisorData,
                    kind: AccessKind::Data,
                    lanes: if parity == 0 {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    },
                };
                let value = if parity == 0 {
                    u16::from(old | 0x80) << 8
                } else {
                    u16::from(old | 0x80)
                };
                assert_eq!(
                    events,
                    [
                        prefetch(0x104, 8),
                        prefetch(0x106, 12),
                        Event::Begin(access, 16),
                        Event::Read(access, 16, 0),
                        Event::Idle(20, 2),
                        Event::Write(access, value, 22, 0),
                        Event::End(26),
                        prefetch(0x108, 26),
                    ]
                );
                assert_eq!(machine.cpu.registers, expected);
                assert_eq!(machine.cpu.queued_words(), Some([0x08d0, 0]));
                assert_eq!(machine.bus.ram, ram);
                assert_eq!(machine.cpu.clock(), CpuClocks(30));
                assert_eq!(machine.bus.cpu_clock, CpuClocks(30));
                assert_eq!(machine.bus.now.get(), 210);
                assert!(machine.bus.rmw.is_none());
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
                machine.encode_state().unwrap();
                let next = machine.step_instruction().unwrap();
                assert_eq!(next.elapsed.get(), 112);
                assert_eq!(machine.cpu.clock(), CpuClocks(46));
                assert_eq!(machine.bus.now.get(), 322);
                assert_eq!(machine.cpu.registers.pc, 0x10a);
                assert_eq!(machine.bus.ram[0x0e + parity as usize], old | 1);
                assert_eq!(
                    machine.bus.ram[0x0f - parity as usize],
                    ram[0x0f - parity as usize]
                );
                assert!(machine.bus.rmw.is_none());
                machine.encode_state().unwrap();
            }
        }
    }
}
