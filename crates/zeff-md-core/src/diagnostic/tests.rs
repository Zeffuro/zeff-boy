use super::*;
use crate::cartridge::Cartridge;
use crate::hardware::bus::Bus;
use context::TraceContext;
use sha2::{Digest, Sha256};
use zeff_m68k::bus::{AccessKind, BusResponse, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

fn machine() -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    Emulator::new(rom, crate::LoadConfig::default()).unwrap()
}

fn access() -> BusAccess {
    BusAccess {
        logical_address: 0xff0021,
        bus_address: 0xff0020,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Lower,
    }
}

struct Mock {
    bus: Bus,
    calls: [u32; 8],
    failure: bool,
    unsupported: bool,
}

impl Mock {
    fn new() -> Self {
        let machine = machine();
        Self {
            bus: Bus::new(
                Cartridge::new(machine.rom_bytes().to_vec()).unwrap(),
                crate::Region::NtscUs,
            ),
            calls: [0; 8],
            failure: false,
            unsupported: false,
        }
    }

    fn call(&mut self, index: usize, start: CpuClocks, transfer: Option<BusAccess>) {
        assert_eq!(start, CpuClocks(11));
        if let Some(transfer) = transfer {
            assert_eq!(transfer, access());
        }
        self.calls[index] += 1;
    }

    fn result<T>(&self, value: T) -> Result<T, BusError> {
        if self.failure {
            Err(BusError::ClockMismatch {
                expected: CpuClocks(7),
                actual: CpuClocks(11),
            })
        } else {
            Ok(value)
        }
    }

    fn response(&self) -> BusResponse<()> {
        if self.unsupported {
            BusResponse::Unsupported
        } else {
            BusResponse::Complete(())
        }
    }
}

impl TraceContext for Mock {
    fn trace_context(&self, target: Option<u32>) -> AdapterContext {
        self.bus.trace_context(target)
    }
}

impl ClockedBus for Mock {
    type Error = BusError;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        self.call(0, start, Some(access));
        CycleResult {
            result: self.result(0x1234),
            wait_clocks: 3,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.call(1, start, Some(access));
        assert_eq!(value, 0xabcd);
        CycleResult {
            result: self.result(()),
            wait_clocks: 4,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.call(2, start, None);
        assert_eq!(clocks, 9);
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        self.call(3, start, Some(access));
        let value = if self.unsupported {
            BusResponse::Unsupported
        } else {
            BusResponse::Complete(InterruptVector::Vector(37))
        };
        CycleResult {
            result: self.result(value),
            wait_clocks: 5,
        }
    }
    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        self.call(4, start, Some(access));
        self.result(self.response())
    }
    fn end_rmw(&mut self, end: CpuClocks) {
        self.call(5, end, None);
    }
    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, BusError> {
        self.call(6, start, None);
        self.result(self.response())
    }
    fn end_reset_output(&mut self, end: CpuClocks) {
        self.call(7, end, None);
    }
}

fn all_callbacks<B: ClockedBus<Error = BusError>>(bus: &mut B) -> [Outcome; 8] {
    let start = CpuClocks(11);
    let read = bus.read_cycle(access(), start);
    assert_eq!(read.wait_clocks, 3);
    let write = bus.write_cycle(access(), 0xabcd, start);
    assert_eq!(write.wait_clocks, 4);
    bus.idle(start, 9);
    let ack = bus.acknowledge_cycle(access(), start);
    assert_eq!(ack.wait_clocks, 5);
    let rmw = bus.begin_rmw(access(), start);
    bus.end_rmw(start);
    let reset = bus.begin_reset_output(start);
    bus.end_reset_output(start);
    [
        read.result
            .map(Outcome::Read)
            .unwrap_or_else(|error| error.diagnostic_outcome()),
        write
            .result
            .map(|()| Outcome::Complete)
            .unwrap_or_else(|error| error.diagnostic_outcome()),
        Outcome::Complete,
        match ack.result {
            Ok(BusResponse::Complete(vector)) => Outcome::Interrupt(vector),
            Ok(BusResponse::Unsupported) => Outcome::Unsupported,
            Err(error) => error.diagnostic_outcome(),
        },
        bus::unit_outcome(&rmw),
        Outcome::Complete,
        bus::unit_outcome(&reset),
        Outcome::Complete,
    ]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_forwards_all_eight_callbacks_and_outcomes() {
    discarded_read_forwarding();
    let terminal = machine();
    let mut hash = Sha256::new();
    for (failure, unsupported) in [(false, false), (false, true), (true, false)] {
        let mut direct = Mock::new();
        let mut traced = Mock::new();
        direct.failure = failure;
        direct.unsupported = unsupported;
        traced.failure = failure;
        traced.unsupported = unsupported;
        let expected = all_callbacks(&mut direct);
        let capture = Capture::begin().unwrap();
        let actual = all_callbacks(&mut TraceBus::new(&mut traced));
        let report = capture.finish(&terminal).unwrap();
        assert!(report.complete);
        assert_eq!(actual, expected);
        assert_eq!(direct.calls, [1; 8]);
        assert_eq!(traced.calls, direct.calls);
        let outcomes: Vec<_> = report
            .history
            .iter()
            .flatten()
            .map(|record| record.outcome.unwrap())
            .collect();
        assert_eq!(outcomes, expected);
        assert_eq!(report.total_callbacks, 8);
        assert_eq!(report.first_failed_sequence, failure.then_some(0));
        hash.update(format!("{report:?}").as_bytes());
    }
    let receipt: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    println!("diagnostic-forward-contract-v1 {receipt}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("diagnostic-forward-contract-v1 {}", receipt);
}

fn discarded_read_forwarding() {
    let terminal = machine();
    for failure in [false, true] {
        let mut direct = Mock::new();
        let mut traced = Mock::new();
        direct.failure = failure;
        traced.failure = failure;
        let expected = direct.read_discarded_cycle(access(), CpuClocks(11));
        let capture = Capture::begin().unwrap();
        let actual = TraceBus::new(&mut traced).read_discarded_cycle(access(), CpuClocks(11));
        let report = capture.finish(&terminal).unwrap();
        assert!(report.complete);
        assert_eq!(actual, expected);
        assert_eq!(actual.wait_clocks, 3);
        assert_eq!(direct.calls, traced.calls);
        assert_eq!(report.total_callbacks, 1);
        let record = report.history[0].unwrap();
        assert_eq!(record.callback, Callback::Read);
        assert_eq!(
            record.outcome,
            Some(if failure {
                Outcome::ClockMismatch {
                    expected: 7,
                    actual: 11,
                }
            } else {
                Outcome::Complete
            })
        );
        assert_eq!(report.first_failed_sequence, failure.then_some(0));
    }
    for concurrent in [false, true] {
        let mut rom = terminal.rom_bytes().to_vec();
        rom[0x200..0x20a].copy_from_slice(&[0x42, 0x39, 0, 0xa1, 0x12, 0, 0x4e, 0x71, 0x60, 0xfe]);
        let mut machine = Emulator::new(
            rom,
            crate::LoadConfig {
                region: crate::Region::NtscUs,
                concurrent_sound: concurrent,
            },
        )
        .unwrap();
        let capture = Capture::begin().unwrap();
        machine.step_instruction().unwrap();
        let report = capture.finish(&machine).unwrap();
        assert!(report.complete);
        assert_eq!(report.first_failed_sequence, None);
        let reads: Vec<_> = report
            .history
            .iter()
            .flatten()
            .filter(|r| {
                r.callback == Callback::Read && r.access.is_some_and(|a| a.bus_address == 0xa11200)
            })
            .collect();
        assert_eq!(reads.len(), 1);
        let read = reads[0];
        assert_eq!(read.access.unwrap().lanes, ByteLanes::Upper);
        assert_eq!(read.outcome, Some(Outcome::Complete));
        assert!(read.service.is_some());
        assert_eq!(read.returned.unwrap().bus.prefetch, Observation::Empty);
        assert_eq!(read.wait_clocks, 0);
        assert_eq!(machine.registers().pc, 0x206);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_scope_drop_nesting_stale_and_reentrant_fail_closed() {
    let terminal = machine();
    let capture = Capture::begin().unwrap();
    assert!(matches!(Capture::begin(), Err(CaptureError::AlreadyActive)));
    let stale = Capture {
        generation: capture.generation + 1,
        finished: false,
        _owner_thread: PhantomData,
    };
    assert!(matches!(stale.finish(&terminal), Err(CaptureError::Stale)));
    assert!(enabled());
    STATE.with(|state| {
        let _borrow = state.borrow_mut();
        instruction_entry(terminal.diagnostic_snapshot());
        assert!(matches!(Capture::begin(), Err(CaptureError::Borrowed)));
    });
    assert!(!capture.finish(&terminal).unwrap().complete);
    let fresh = Capture::begin().unwrap();
    assert!(fresh.finish(&terminal).unwrap().complete);
    let dropped = Capture::begin().unwrap();
    drop(dropped);
    assert!(!enabled());
    let last = Capture::begin().unwrap();
    assert!(last.finish(&terminal).unwrap().complete);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_ring_order_and_checked_counter_limits_preserve_forwarding() {
    let terminal = machine();
    let mut mock = Mock::new();
    let capture = Capture::begin().unwrap();
    for _ in 0..70 {
        TraceBus::new(&mut mock).idle(CpuClocks(11), 9);
    }
    let report = capture.finish(&terminal).unwrap();
    assert!(report.complete);
    assert_eq!(report.overwritten, 6);
    for (index, record) in report.history.iter().enumerate() {
        assert_eq!(record.unwrap().sequence, index as u64 + 6);
    }
    assert_eq!(mock.calls[2], 70);
    let capture = Capture::begin().unwrap();
    observe(|recorder| recorder.next = u64::MAX);
    TraceBus::new(&mut mock).idle(CpuClocks(11), 9);
    assert!(!capture.finish(&terminal).unwrap().complete);
    assert_eq!(mock.calls[2], 71);
    let generation = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let generation = state.generation;
        state.generation = u64::MAX;
        generation
    });
    assert!(matches!(
        Capture::begin(),
        Err(CaptureError::CounterExhausted)
    ));
    STATE.with(|state| state.borrow_mut().generation = generation);
    let capture = Capture::begin().unwrap();
    assert!(capture.finish(&terminal).unwrap().complete);
}

mod instruction_origin;
