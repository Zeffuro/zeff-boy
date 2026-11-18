use super::*;

#[derive(Default)]
struct ReceiptSpy {
    inner: Spy,
    collected: usize,
}

impl ClockedBus for ReceiptSpy {
    type Error = &'static str;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, Self::Error> {
        self.inner.read_cycle(access, start)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), Self::Error> {
        self.inner.write_cycle(access, value, start)
    }

    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), Self::Error> {
        self.inner.write_cycle_observed(access, value, start)
    }

    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), Self::Error>, [Option<u8>; 2]) {
        self.collected += 1;
        (
            self.write_cycle(access, value, start),
            [Some(0x81), Some(0x92)],
        )
    }

    fn write_cycle_observed_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (ObservedCycle<(), Self::Error>, [Option<u8>; 2]) {
        self.collected += 1;
        (
            self.write_cycle_observed(access, value, start),
            [Some(0x81), Some(0x92)],
        )
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.inner.idle(start, clocks);
    }
}

#[derive(Default)]
struct Demand {
    previous: bool,
    writes: Vec<(BusAccess, u16)>,
    memory: Vec<MemoryAccess>,
}

impl InstructionObserver for Demand {
    fn needs_previous_values(&self) -> bool {
        self.previous
    }

    fn write(&mut self, access: BusAccess, value: u16) {
        self.writes.push((access, value));
    }

    fn memory_access(&mut self, access: MemoryAccess) {
        self.memory.push(access);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn observer_demand_selects_one_write_and_error_receipts_emit_no_callbacks() {
    for previous in [false, true] {
        for fail in [false, true] {
            for observed in [false, true] {
                let mut spy = ReceiptSpy::default();
                spy.inner.fail = fail;
                let mut demand = Demand {
                    previous,
                    ..Demand::default()
                };
                let access = access(AccessKind::Data, ByteLanes::Both);
                let mut bus = ObservedBus {
                    inner: &mut spy,
                    observer: &mut demand,
                };
                let cycle = if observed {
                    let result = bus.write_cycle_observed(access, 0xa55a, CpuClocks(37));
                    assert_eq!(result.samples.unwrap().levels(), (3, 5));
                    result.cycle
                } else {
                    bus.write_cycle(access, 0xa55a, CpuClocks(37))
                };
                assert_eq!(cycle.result, if fail { Err("fault") } else { Ok(()) });
                assert_eq!(cycle.wait_clocks, 7);
                assert_eq!(spy.collected, usize::from(previous));
                assert_eq!(
                    spy.inner.calls,
                    [(
                        if observed { "observed write" } else { "write" },
                        CpuClocks(37),
                        Some(access),
                        0xa55a
                    )]
                );
                if fail {
                    assert!(demand.writes.is_empty() && demand.memory.is_empty());
                } else {
                    assert_eq!(demand.writes, [(access, 0xa55a)]);
                    assert_eq!(
                        demand
                            .memory
                            .iter()
                            .map(|event| (
                                event.address,
                                event.value,
                                event.old_value,
                                event.direction
                            ))
                            .collect::<Vec<_>>(),
                        [
                            (
                                0xff0040,
                                Some(0xa5),
                                previous.then_some(0x81),
                                Direction::Write
                            ),
                            (
                                0xff0041,
                                Some(0x5a),
                                previous.then_some(0x92),
                                Direction::Write
                            )
                        ]
                    );
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fn_mut_memory_observers_keep_previous_value_demand_by_default() {
    for observed in [false, true] {
        let mut spy = ReceiptSpy::default();
        let mut events = Vec::new();
        let mut observer = |event| events.push(event);
        assert!(observer.needs_previous_values());
        let mut bus = ObservedBus {
            inner: &mut spy,
            observer: &mut observer,
        };
        let access = access(AccessKind::Stack, ByteLanes::Both);
        if observed {
            assert_eq!(
                bus.write_cycle_observed(access, 0x1234, CpuClocks(11))
                    .samples
                    .unwrap()
                    .levels(),
                (3, 5)
            );
        } else {
            assert_eq!(
                bus.write_cycle(access, 0x1234, CpuClocks(11)).result,
                Ok(())
            );
        }
        assert_eq!(spy.collected, 1);
        assert_eq!(spy.inner.calls.len(), 1);
        assert_eq!(
            events
                .iter()
                .map(|event| event.old_value)
                .collect::<Vec<_>>(),
            [Some(0x81), Some(0x92)]
        );
    }
}
