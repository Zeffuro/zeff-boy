use super::*;

struct MatrixProbe<'a, 'b> {
    inner: ScheduledBus<'a>,
    case: &'b Case,
    expected: Option<super::super::oracle::Oracle>,
    controls: Option<&'b [Snapshot]>,
    session: Option<&'b Session>,
    captures: Vec<Snapshot>,
    position: usize,
    baseline: Option<Report>,
    original: Option<u64>,
    checkpoint: Option<u64>,
}

impl MatrixProbe<'_, '_> {
    fn point(&mut self) {
        if let Some(expected) = &self.expected {
            expected.check(&self.inner);
        }
        if let Some(controls) = self.controls {
            controls[self.position].check(&self.inner);
        } else {
            assert!(self.session.is_none());
            self.captures.push(Snapshot::capture(&self.inner));
        }
        self.position += 1;
    }
}

impl ClockedBus for MatrixProbe<'_, '_> {
    type Error = BusError;

    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        assert_eq!(self.position, 0);
        assert_eq!(a, access(self.case.lane));
        assert_eq!(t.0 * 7, self.case.a());
        let result = self.inner.begin_rmw(a, t);
        assert_eq!(result, Ok(BusResponse::Complete(())));
        self.point();
        result
    }

    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        assert_eq!(self.position, 1);
        assert_eq!(a, access(self.case.lane));
        let wait = self.expected.as_mut().map(|expected| expected.read());
        let result = self.inner.read_cycle(a, t);
        assert_eq!(
            result.result,
            Ok(if self.case.lane == 0 { 0x00de } else { 0xde00 })
        );
        if let Some(wait) = wait {
            assert_eq!(result.wait_clocks, wait);
        }
        self.original = self
            .inner
            .schedule
            .pending
            .as_ref()
            .map(|p| p.observer_image_id());
        self.checkpoint = self
            .inner
            .schedule
            .rmw_checkpoint
            .as_ref()
            .map(|p| p.observer_image_id());
        let held = self.case.s() >= self.case.a();
        assert_eq!(self.checkpoint.is_some(), held);
        if let (Some(session), Some(baseline)) = (self.session, &self.baseline) {
            if held {
                assert_ne!(self.original, Some(0));
                assert_ne!(self.checkpoint, Some(0));
                assert_ne!(self.original, self.checkpoint);
            }
            let report = session.report();
            assert_eq!(report.flags, 0);
            assert_eq!(
                report.count(Metric::ImageClones) - baseline.count(Metric::ImageClones),
                u64::from(held)
            );
            assert_eq!(
                report.count(Metric::DeepCopies) - baseline.count(Metric::DeepCopies),
                u64::from(held)
            );
        }
        self.point();
        result
    }

    fn idle(&mut self, t: CpuClocks, clocks: u32) {
        assert_eq!(self.position, 2);
        assert_eq!(clocks, 2);
        if let Some(expected) = &mut self.expected {
            expected.held(14);
        }
        self.inner.idle(t, clocks);
        self.point();
    }

    fn write_cycle(&mut self, a: BusAccess, value: u16, _: CpuClocks) -> CycleResult<(), BusError> {
        assert_eq!(self.position, 3);
        assert_eq!(a, access(self.case.lane));
        assert_eq!(value, if self.case.lane == 0 { 0x8000 } else { 0x80 });
        self.point();
        CycleResult {
            result: Err(BusError::UnsupportedAccess {
                address: a.bus_address,
                direction: zeff_m68k::bus::Direction::Write,
            }),
            wait_clocks: 0,
        }
    }

    fn end_rmw(&mut self, t: CpuClocks) {
        assert_eq!(self.position, 4);
        assert_eq!(t.0, self.inner.bus.cpu_clock.0 + 4);
        if let Some(expected) = &mut self.expected {
            expected.end();
        }
        let before = self.session.map(Session::report);
        self.inner.end_rmw(t);
        assert!(self.inner.bus.rmw.is_none());
        assert!(self.inner.schedule.rmw_checkpoint.is_none());
        if let Some(checkpoint) = self.checkpoint {
            assert_eq!(
                self.inner
                    .schedule
                    .pending
                    .as_ref()
                    .unwrap()
                    .observer_image_id(),
                checkpoint
            );
        }
        if let (Some(session), Some(mut expected)) = (self.session, before) {
            if self.checkpoint.is_some() {
                expected.live_owners -= 1;
                expected.live_images -= 1;
                expected.counters[Metric::OwnersClosed as usize] += 1;
                expected.counters[Metric::ImagesClosed as usize] += 1;
            }
            assert_eq!(session.report(), expected);
        }
        self.point();
    }
}

struct Control {
    fault: TimingFault<BusError>,
    cpu: PrefetchCpu,
    callbacks: Vec<Snapshot>,
    fences: Vec<Snapshot>,
    deadline: u64,
    ends: Vec<u64>,
    held: bool,
    pcm: Vec<u32>,
}

fn pcm(machine: &mut Emulator) -> Vec<u32> {
    let mut samples = vec![];
    machine.bus.audio.drain_into(&mut samples);
    samples.into_iter().map(f32::to_bits).collect()
}

fn control(case: &Case) -> Control {
    let (mut machine, expected) = seed(true, case);
    let mut probe = MatrixProbe {
        inner: ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ),
        case,
        expected,
        controls: None,
        session: None,
        captures: vec![],
        position: 0,
        baseline: None,
        original: None,
        checkpoint: None,
    };
    let fault = machine.cpu.step_instruction(&mut probe).unwrap_err();
    assert_eq!(probe.position, 5);
    let callbacks = std::mem::take(&mut probe.captures);
    let mut expected = probe.expected.take().unwrap();
    drop(probe);
    let deadline = expected.target.unwrap().1;
    let held = case.s() >= case.a();
    assert_eq!(deadline > machine.bus.now.get(), held);
    let ends = if held {
        vec![deadline - 1, deadline, deadline + 1, deadline + 180]
    } else {
        vec![machine.bus.now.get() + 1, machine.bus.now.get() + 180]
    };
    let mut fences = vec![];
    for &end in &ends {
        expected.future(end);
        let ticks = end - machine.bus.now.get();
        advance(&mut machine, ticks);
        expected.check(&adapter(&mut machine).inner);
        fences.push(Snapshot::capture(&adapter(&mut machine).inner));
    }
    assert_eq!(expected.target_publications, 1);
    Control {
        fault,
        cpu: machine.cpu.clone(),
        callbacks,
        fences,
        deadline,
        ends,
        held,
        pcm: pcm(&mut machine),
    }
}

fn observe(case: &Case, control: &Control) -> Report {
    let session = Session::begin();
    {
        let (mut machine, oracle) = seed(true, case);
        drop(oracle);
        let baseline = session.report();
        let checkpoint = {
            let mut probe = MatrixProbe {
                inner: ScheduledBus::new(
                    &mut machine.bus,
                    &mut machine.sound_cpu,
                    &mut machine.sound_schedule,
                ),
                case,
                expected: None,
                controls: Some(&control.callbacks),
                session: Some(&session),
                captures: vec![],
                position: 0,
                baseline: Some(baseline),
                original: None,
                checkpoint: None,
            };
            assert_eq!(
                machine.cpu.step_instruction(&mut probe),
                Err(control.fault.clone())
            );
            assert_eq!(probe.position, 5);
            probe.checkpoint
        };
        assert_eq!(machine.cpu, control.cpu);
        frozen_retry(&mut machine, &session);
        for (index, &end) in control.ends.iter().enumerate() {
            let before = session.report().count(Metric::Publications);
            let ticks = end - machine.bus.now.get();
            advance(&mut machine, ticks);
            control.fences[index].check(&adapter(&mut machine).inner);
            if control.held && index == 0 {
                assert_eq!(
                    machine
                        .sound_schedule
                        .pending
                        .as_ref()
                        .unwrap()
                        .observer_image_id(),
                    checkpoint.unwrap()
                );
                assert_eq!(machine.bus.ram[0x800e + case.lane as usize], 0xde);
            } else {
                assert_eq!(machine.bus.ram[0x800e + case.lane as usize], 0xa6);
            }
            if control.held && index == 1 {
                assert_eq!(session.report().count(Metric::Publications), before + 1);
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
            }
        }
        assert_eq!(pcm(&mut machine), control.pcm);
    }
    let report = session.finish();
    assert!(report.closed(), "{case:?}: {report:?}");
    assert_eq!(
        report.count(Metric::OwnersCreated),
        report.count(Metric::OwnersClosed)
    );
    assert_eq!(
        report.count(Metric::ImagesCreated),
        report.count(Metric::ImagesClosed)
    );
    assert_eq!(
        report.count(Metric::Attempts),
        report.count(Metric::Successes) + report.count(Metric::Failures)
    );
    assert!(!report.full_coverage);
    assert_eq!(report.scope, "DirectedOwnerThreadOnly");
    report
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_actual_rmw_checkpoint_residues_edges_lanes_and_publication_fences() {
    let mut hash = 0xcbf29ce484222325u64;
    let mut cases = 0;
    let mut held_cases = 0;
    for residue in 0..15 {
        for edge in [-1, 0, 1] {
            for lane in [0, 1] {
                let case = Case {
                    kind: Kind::Immediate,
                    residue,
                    edge,
                    lane,
                };
                let expected = control(&case);
                let report = observe(&case, &expected);
                held_cases += usize::from(expected.held);
                for byte in format!("{case:?}|{}|{report:?}", expected.deadline).bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 90);
    assert_eq!(held_cases, 88);
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("md-observer-rmw-matrix-v1 {hash:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("md-observer-rmw-matrix-v1 {hash:016x} cases={cases}");
}
