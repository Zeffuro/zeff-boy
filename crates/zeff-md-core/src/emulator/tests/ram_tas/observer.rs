use super::common::*;
use super::*;
use crate::hardware::sound::observer::{Metric, Report, Session, Verification};

#[path = "observer/forward.rs"]
mod forward;
use forward::{Probe, Snapshot};

#[path = "observer/matrix.rs"]
mod matrix;

fn seeded() -> (Emulator, super::oracle::Oracle) {
    let case = Case {
        kind: Kind::Immediate,
        residue: 0,
        edge: 0,
        lane: 0,
    };
    assert_eq!(
        (case.nops(), case.a(), case.s(), case.origin()),
        (43, 1260, 1260, 600)
    );
    let (machine, expected) = seed(true, &case);
    assert!(machine.sound_schedule.pending.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    assert!(machine.bus.rmw.is_none());
    assert_eq!(machine.cpu.clock(), CpuClocks(180));
    assert_eq!(machine.bus.cpu_clock, CpuClocks(180));
    (machine, expected.unwrap())
}

struct Execution {
    fault: TimingFault<BusError>,
    captures: Vec<Snapshot>,
    checkpoint: u64,
    expected: Option<super::oracle::Oracle>,
}

fn execute_abort(
    machine: &mut Emulator,
    expected: Option<super::oracle::Oracle>,
    controls: Option<&[Snapshot]>,
    session: Option<&Session>,
) -> Execution {
    let registers = machine.cpu.registers;
    let mut probe = Probe {
        inner: ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ),
        expected,
        controls,
        session,
        captures: Vec::new(),
        position: 0,
        original: 0,
        checkpoint: 0,
    };
    let fault = machine.cpu.step_instruction(&mut probe).unwrap_err();
    assert_eq!(probe.position, 5);
    let execution = Execution {
        fault,
        captures: probe.captures,
        checkpoint: probe.checkpoint,
        expected: probe.expected,
    };
    assert_eq!(machine.cpu.clock(), CpuClocks(190));
    assert_eq!(machine.bus.cpu_clock, CpuClocks(186));
    assert_eq!(machine.bus.now.get(), 1302);
    assert_eq!(machine.cpu.registers.pc, registers.pc);
    assert_eq!(machine.cpu.registers.status_register(), 0x2714);
    assert_eq!(machine.cpu.queued_words(), Some([0x4ad0, 0x4e71]));
    execution
}

fn frozen_retry(machine: &mut Emulator, session: &Session) {
    struct Rejected;
    impl ClockedBus for Rejected {
        type Error = BusError;
        fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, BusError> {
            panic!("frozen read")
        }
        fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), BusError> {
            panic!("frozen write")
        }
        fn idle(&mut self, _: CpuClocks, _: u32) {
            panic!("frozen idle")
        }
        fn begin_rmw(&mut self, _: BusAccess, _: CpuClocks) -> Result<BusResponse<()>, BusError> {
            panic!("frozen begin")
        }
        fn end_rmw(&mut self, _: CpuClocks) {
            panic!("frozen end")
        }
    }
    let before = session.report();
    let cpu = machine.cpu.clone();
    assert_eq!(
        machine.cpu.step_instruction(&mut Rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(
        machine.cpu.step_boundary(&mut Rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(
        machine.cpu.service_pending_trace(&mut Rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(machine.cpu, cpu);
    assert_eq!(session.report(), before);
}

fn work(report: &Report) {
    assert_eq!(report.flags, 0);
    assert_eq!(report.count(Metric::Attempts), 1);
    assert_eq!(report.count(Metric::Successes), 1);
    assert_eq!(report.count(Metric::Failures), 0);
    assert_eq!(report.count(Metric::DeepCopies), 1);
    assert_eq!(report.count(Metric::CopyBytes), 8192);
    assert_eq!(report.count(Metric::ImageClones), 1);
    assert_eq!(report.count(Metric::ImagesCreated), 2);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_actual_main_tas_h_abort_0005() {
    let (mut control, expected) = seeded();
    let control_execution = execute_abort(&mut control, Some(expected), None, None);
    let mut expected = control_execution.expected.unwrap();
    let control_cpu = control.cpu.clone();
    let mut fences = Vec::new();
    for deadline in [1349, 1350, 1530] {
        expected.future(deadline);
        let ticks = deadline - control.bus.now.get();
        advance(&mut control, ticks);
        expected.check(&adapter(&mut control).inner);
        fences.push(Snapshot::capture(&adapter(&mut control).inner));
    }
    assert_eq!(expected.target_publications, 1);
    let (mut machine, unused_control) = seeded();
    drop(unused_control);
    let session = Session::begin();
    machine.bus.sound.ram.enroll_observer();
    let entry = session.report();
    assert_eq!(entry.flags, 0);
    assert_eq!((entry.live_owners, entry.live_images), (1, 0));
    let execution = execute_abort(
        &mut machine,
        None,
        Some(&control_execution.captures),
        Some(&session),
    );
    assert_eq!(execution.fault, control_execution.fault);
    assert_eq!(machine.cpu, control_cpu);
    frozen_retry(&mut machine, &session);
    let after_end = session.report();
    work(&after_end);
    assert_eq!(after_end.count(Metric::ImagesClosed), 1);
    assert_eq!(after_end.count(Metric::Publications), 0);
    advance(&mut machine, 47);
    fences[0].check(&adapter(&mut machine).inner);
    assert_eq!(machine.bus.ram[0x0e], 0);
    assert_eq!(machine.bus.ram[0x800e], 0xde);
    assert_eq!(
        machine
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .observer_image_id(),
        execution.checkpoint
    );
    assert_eq!(session.report(), after_end);
    advance(&mut machine, 1);
    fences[1].check(&adapter(&mut machine).inner);
    assert_eq!(machine.bus.ram[0x0e], 0);
    assert_eq!(machine.bus.ram[0x800e], 0xa6);
    assert!(machine.sound_schedule.pending.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    let at1350 = session.report();
    work(&at1350);
    assert_eq!(at1350.count(Metric::ImagesClosed), 2);
    assert_eq!(at1350.count(Metric::Publications), 1);
    assert_eq!(at1350.live_owners, 1);
    assert_eq!(at1350.live_images, 0);
    assert_eq!(
        at1350.counters,
        [
            4, 3, 2, 2, 1, 0, 0, 0, 8192, 2, 2, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0
        ]
    );
    advance(&mut machine, 180);
    fences[2].check(&adapter(&mut machine).inner);
    drop(machine);
    let report = session.finish();
    assert!(report.closed(), "{report:?}");
    assert!(!report.full_coverage);
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("OBSERVER_TAS_0005_AT1350={at1350:?}; FINAL1530={report:?}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("OBSERVER_TAS_0005_AT1350={at1350:?}; FINAL1530={report:?}");
}
