use super::*;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::InterruptSamples;

mod demand;
mod instruction_trace;
mod machine;
mod precise_previous;
mod previous;

#[derive(Default)]
struct Spy {
    calls: Vec<(&'static str, CpuClocks, Option<BusAccess>, u32)>,
    words: Vec<(u32, u16)>,
    fail: bool,
}

impl Spy {
    fn cycle<T>(&mut self, value: T) -> CycleResult<T, &'static str> {
        CycleResult {
            result: if self.fail { Err("fault") } else { Ok(value) },
            wait_clocks: 7,
        }
    }
}

impl ClockedBus for Spy {
    type Error = &'static str;

    fn instruction_word(&mut self, address: u32, value: u16) {
        self.words.push((address, value));
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.calls.push(("read", start, Some(access), 0));
        self.cycle(0xa55a)
    }

    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<u16, Self::Error> {
        assert_eq!(aperture, InterruptAperture::HandlerRefill);
        self.calls.push(("observed read", start, Some(access), 0));
        ObservedCycle {
            cycle: self.cycle(0xa55a),
            samples: InterruptSamples::new(2, 4),
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.calls
            .push(("write", start, Some(access), value.into()));
        self.cycle(())
    }

    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), Self::Error> {
        self.calls
            .push(("observed write", start, Some(access), value.into()));
        ObservedCycle {
            cycle: self.cycle(()),
            samples: InterruptSamples::new(3, 5),
        }
    }

    fn read_discarded_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.calls.push(("discard", start, Some(access), 0));
        self.cycle(())
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.calls.push(("idle", start, None, clocks));
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, Self::Error> {
        self.calls.push(("ack", start, Some(access), 0));
        self.cycle(BusResponse::Complete(InterruptVector::Autovector))
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, Self::Error> {
        self.calls.push(("begin rmw", start, Some(access), 0));
        self.cycle(BusResponse::Complete(())).result
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        self.calls.push(("end rmw", end, None, 0));
    }

    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, Self::Error> {
        self.calls.push(("begin reset", start, None, 0));
        self.cycle(BusResponse::Complete(())).result
    }

    fn end_reset_output(&mut self, end: CpuClocks) {
        self.calls.push(("end reset", end, None, 0));
    }
}

fn access(kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: 0x01ff0041,
        bus_address: 0xff0040,
        function_code: FunctionCode::SupervisorProgram,
        kind,
        lanes,
    }
}

fn exercise(bus: &mut impl ClockedBus<Error = &'static str>, fail: bool) {
    bus.instruction_word(0x1234_5678, 0xa55a);
    let access = access(AccessKind::Data, ByteLanes::Both);
    let start = CpuClocks(37);
    let unit = if fail { Err("fault") } else { Ok(()) };
    let word = if fail { Err("fault") } else { Ok(0xa55a) };
    assert_eq!(
        bus.read_cycle(access, start),
        CycleResult {
            result: word,
            wait_clocks: 7
        }
    );
    assert_eq!(
        bus.write_cycle(access, 0x1234, start),
        CycleResult {
            result: unit,
            wait_clocks: 7
        }
    );
    let read = bus.read_cycle_observed(access, start, InterruptAperture::HandlerRefill);
    assert_eq!(read.cycle.wait_clocks, 7);
    assert_eq!(read.cycle.result, word);
    assert_eq!(read.samples.unwrap().levels(), (2, 4));
    let write = bus.write_cycle_observed(access, 0x5678, start);
    assert_eq!(write.cycle.wait_clocks, 7);
    assert_eq!(write.cycle.result, unit);
    assert_eq!(write.samples.unwrap().levels(), (3, 5));
    assert_eq!(
        bus.read_discarded_cycle(access, start),
        CycleResult {
            result: unit,
            wait_clocks: 7
        }
    );
    assert_eq!(
        bus.acknowledge_cycle(access, start),
        CycleResult {
            result: unit.map(|()| BusResponse::Complete(InterruptVector::Autovector)),
            wait_clocks: 7
        }
    );
    assert_eq!(
        bus.begin_rmw(access, start),
        unit.map(|()| BusResponse::Complete(()))
    );
    bus.end_rmw(CpuClocks(45));
    assert_eq!(
        bus.begin_reset_output(start),
        unit.map(|()| BusResponse::Complete(()))
    );
    bus.end_reset_output(CpuClocks(169));
    bus.idle(start, 17);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn every_hook_preserves_results_samples_arguments_and_single_delegation() {
    for fail in [false, true] {
        let mut expected = Spy {
            fail,
            ..Spy::default()
        };
        exercise(&mut expected, fail);
        let mut actual = Spy {
            fail,
            ..Spy::default()
        };
        let mut events = Vec::new();
        exercise(
            &mut ObservedBus {
                inner: &mut actual,
                observer: &mut |event| events.push(event),
            },
            fail,
        );
        assert!(events.iter().all(|event| event.old_value.is_none()));
        assert_eq!(actual.calls, expected.calls);
        assert_eq!(actual.words, [(0x1234_5678, 0xa55a)]);
        assert_eq!(actual.words, expected.words);
        assert_eq!(events.len(), if fail { 0 } else { 10 });
        if !fail {
            assert_eq!(
                events.iter().map(|event| event.value).collect::<Vec<_>>(),
                [
                    Some(0xa5),
                    Some(0x5a),
                    Some(0x12),
                    Some(0x34),
                    Some(0xa5),
                    Some(0x5a),
                    Some(0x56),
                    Some(0x78),
                    None,
                    None
                ]
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn kind_filter_and_physical_lanes_include_program_space_operands_only() {
    let mut spy = Spy::default();
    let mut events = Vec::new();
    for kind in [
        AccessKind::Opcode,
        AccessKind::Extension,
        AccessKind::ResetVector,
        AccessKind::Vector,
        AccessKind::Stack,
        AccessKind::Data,
        AccessKind::Discard,
        AccessKind::Prefetch,
        AccessKind::InterruptAcknowledge,
    ] {
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let mut observer = |event| events.push(event);
            let mut bus = ObservedBus {
                inner: &mut spy,
                observer: &mut observer,
            };
            assert_eq!(
                bus.read_cycle(access(kind, lanes), CpuClocks(1)).result,
                Ok(0xa55a)
            );
        }
    }
    assert_eq!(events.len(), 8);
    for four in events.as_chunks::<4>().0 {
        assert_eq!(
            four.iter()
                .map(|event| (event.address, event.value))
                .collect::<Vec<_>>(),
            [
                (0xff0040, Some(0xa5)),
                (0xff0041, Some(0x5a)),
                (0xff0040, Some(0xa5)),
                (0xff0041, Some(0x5a))
            ]
        );
        assert!(four.iter().all(|event| event.direction == Direction::Read));
    }
}

mod origin;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn previous_value_defaults_preserve_custom_samples_faults_waits_and_one_call() {
    for fail in [false, true] {
        let mut bus = Spy {
            fail,
            ..Spy::default()
        };
        let access = access(AccessKind::Data, ByteLanes::Both);
        let (plain, previous) = bus.write_cycle_with_previous(access, 0x1234, CpuClocks(37));
        assert_eq!(previous, [None, None]);
        assert_eq!(plain.result, if fail { Err("fault") } else { Ok(()) });
        assert_eq!(plain.wait_clocks, 7);
        assert_eq!(bus.calls, [("write", CpuClocks(37), Some(access), 0x1234)]);
        bus.calls.clear();
        let (observed, previous) =
            bus.write_cycle_observed_with_previous(access, 0x5678, CpuClocks(39));
        assert_eq!(previous, [None, None]);
        assert_eq!(observed.cycle.result, plain.result);
        assert_eq!(observed.cycle.wait_clocks, 7);
        assert_eq!(observed.samples.unwrap().levels(), (3, 5));
        assert_eq!(
            bus.calls,
            [("observed write", CpuClocks(39), Some(access), 0x5678)]
        );
    }
}
