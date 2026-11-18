use super::*;

pub(super) struct Snapshot {
    bus: Bus,
    cpu: zeff_z80::Cpu,
    schedule: SoundSchedule,
}

impl Snapshot {
    pub fn capture(bus: &ScheduledBus<'_>) -> Self {
        Self {
            bus: bus.bus.clone(),
            cpu: bus.cpu.clone(),
            schedule: bus.schedule.clone(),
        }
    }

    pub fn check(&self, bus: &ScheduledBus<'_>) {
        let _verification = Verification::begin();
        assert_eq!(*bus.bus, self.bus);
        assert_eq!(*bus.cpu, self.cpu);
        assert_eq!(*bus.schedule, self.schedule);
        assert_eq!(
            format!("{:?}", bus.schedule),
            format!("{:?}", self.schedule)
        );
    }
}

pub(super) struct Probe<'a, 'b> {
    pub inner: ScheduledBus<'a>,
    pub expected: Option<super::super::oracle::Oracle>,
    pub controls: Option<&'b [Snapshot]>,
    pub session: Option<&'b Session>,
    pub captures: Vec<Snapshot>,
    pub position: usize,
    pub original: u64,
    pub checkpoint: u64,
}

impl Probe<'_, '_> {
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

impl ClockedBus for Probe<'_, '_> {
    type Error = BusError;

    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        assert_eq!(self.position, 0);
        assert_eq!(a, access(0));
        assert_eq!(t, CpuClocks(180));
        let result = self.inner.begin_rmw(a, t);
        assert_eq!(result, Ok(BusResponse::Complete(())));
        self.point();
        result
    }

    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        assert_eq!(self.position, 1);
        assert_eq!(a, access(0));
        assert_eq!(t, CpuClocks(180));
        if let Some(expected) = &mut self.expected {
            assert_eq!(expected.read(), 0);
        }
        let result = self.inner.read_cycle(a, t);
        assert_eq!(result, CycleResult::ready(0x00de));
        assert_eq!(self.inner.bus.now.get(), 1288);
        let pending = self.inner.schedule.pending.as_ref().unwrap();
        let checkpoint = self.inner.schedule.rmw_checkpoint.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (1335, 1380));
        assert_eq!((checkpoint.start, checkpoint.deadline), (1260, 1305));
        self.original = pending.observer_image_id();
        self.checkpoint = checkpoint.observer_image_id();
        if let Some(session) = self.session {
            assert_ne!(self.original, 0);
            assert_ne!(self.checkpoint, 0);
            assert_ne!(self.original, self.checkpoint);
            let report = session.report();
            assert_eq!(report.flags, 0);
            assert_eq!((report.live_owners, report.live_images), (3, 2));
            assert_eq!(report.count(Metric::Attempts), 1);
            assert_eq!(report.count(Metric::Successes), 1);
            assert_eq!(report.count(Metric::Failures), 0);
            assert_eq!(report.count(Metric::ImagesCreated), 2);
            assert_eq!(report.count(Metric::ImageClones), 1);
            assert_eq!(report.count(Metric::ImagesClosed), 0);
            assert_eq!(report.count(Metric::DeepCopies), 1);
            assert_eq!(report.count(Metric::CopyBytes), 8192);
            assert_eq!(report.count(Metric::SharedDetaches), 0);
            assert_eq!(report.count(Metric::Publications), 0);
        }
        self.point();
        result
    }

    fn idle(&mut self, t: CpuClocks, clocks: u32) {
        assert_eq!(self.position, 2);
        assert_eq!((t, clocks), (CpuClocks(184), 2));
        if let Some(expected) = &mut self.expected {
            expected.held(14);
        }
        self.inner.idle(t, clocks);
        assert_eq!(self.inner.bus.now.get(), 1302);
        self.point();
    }

    fn write_cycle(&mut self, a: BusAccess, value: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        assert_eq!(self.position, 3);
        assert_eq!(a, access(0));
        assert_eq!((value, t), (0x8000, CpuClocks(186)));
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
        assert_eq!(t, CpuClocks(190));
        assert_eq!(self.inner.bus.cpu_clock, CpuClocks(186));
        assert_eq!(self.inner.bus.now.get(), 1302);
        if let Some(expected) = &mut self.expected {
            expected.end();
        }
        let before = self.session.map(Session::report);
        self.inner.end_rmw(t);
        assert!(self.inner.bus.rmw.is_none());
        assert!(self.inner.schedule.rmw_checkpoint.is_none());
        let pending = self.inner.schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (1305, 1350));
        assert_eq!(pending.observer_image_id(), self.checkpoint);
        if let (Some(session), Some(mut before)) = (self.session, before) {
            before.live_owners -= 1;
            before.live_images -= 1;
            before.counters[Metric::OwnersClosed as usize] += 1;
            before.counters[Metric::ImagesClosed as usize] += 1;
            assert_eq!(session.report(), before);
        }
        self.point();
    }
}
