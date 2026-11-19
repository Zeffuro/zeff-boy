use crate::device::Device as Ordinary;
use crate::fixed::Fixed;
use crate::literal::Value;
use zeff_m68k::bus::{BusAccess, Direction};
use zeff_m68k::timing::physical::{CriticalFault, EntryBus, EntryFault, EntryOutcome, EntryReport};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

pub const PHASES: [&str; 6] = [
    "ResetSPHigh",
    "ResetSPLow",
    "ResetPCHigh",
    "ResetPCLow",
    "HandlerFirst",
    "HandlerSecond",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Complete,
    HostFailure,
    CriticalAbortReleased,
    TimingUnavailable,
}
impl Kind {
    pub fn parse(name: &str) -> Self {
        match name {
            "Complete" => Self::Complete,
            "HostFailure" => Self::HostFailure,
            "CriticalAbortReleased" => Self::CriticalAbortReleased,
            "TimingUnavailable" => Self::TimingUnavailable,
            _ => panic!("reset kind"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Complete => "Complete",
            Self::HostFailure => "HostFailure",
            Self::CriticalAbortReleased => "CriticalAbortReleased",
            Self::TimingUnavailable => "TimingUnavailable",
        }
    }
    fn outcome<T>(self, value: T, index: usize) -> EntryOutcome<T, usize> {
        match self {
            Self::Complete => EntryOutcome::Complete(value),
            Self::HostFailure => EntryOutcome::HostFailure(index),
            Self::CriticalAbortReleased => EntryOutcome::CriticalAbortReleased,
            Self::TimingUnavailable => EntryOutcome::TimingUnavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub kind: Kind,
    pub elapsed: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detector {
    pub report: Report,
    pub omitted: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detection {
    pub phase: &'static str,
    pub access: BusAccess,
    pub start: u64,
    pub detector: Detector,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Idle {
        phase: &'static str,
        start: u64,
        elapsed: u32,
    },
    Transfer {
        phase: &'static str,
        access: BusAccess,
        value: u16,
        start: u64,
        report: Report,
        index: usize,
    },
    Detection(Detection),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Script {
    pub phase: &'static str,
    pub access: BusAccess,
    pub value: u16,
    pub report: Report,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub ordinary: Ordinary,
    pub reports: Fixed<Report>,
    pub detector: Detector,
    pub events: Fixed<Event>,
    pub scripts: Fixed<Script>,
    pub defaults: Fixed<Detection>,
    pub transfers: usize,
    pub detectors: usize,
    pub idles: usize,
}

impl Device {
    pub fn new(ordinary: Ordinary, request: &Value) -> Self {
        let mut reports = Fixed::default();
        let durations = request.find("durations");
        for key in ["0", "1", "2", "3", "4", "5"] {
            reports.push(Report {
                kind: Kind::Complete,
                elapsed: durations.and_then(|v| v.find(key)).map_or(4, Value::number) as u32,
            });
        }
        if let Some(selected) = request.find("report") {
            reports.slots[selected.get("at").number() as usize] = Some(Report {
                kind: Kind::parse(selected.get("outcome").text()),
                elapsed: selected.get("elapsed").number() as u32,
            });
        }
        let detector = request.find("detector").map_or(
            Detector {
                report: Report {
                    kind: Kind::TimingUnavailable,
                    elapsed: 0,
                },
                omitted: true,
            },
            |value| Detector {
                report: Report {
                    kind: Kind::parse(value.get("outcome").text()),
                    elapsed: value.get("elapsed").number() as u32,
                },
                omitted: value.get("omitted").boolean(),
            },
        );
        Self {
            ordinary,
            reports,
            detector,
            events: Fixed::default(),
            scripts: Fixed::default(),
            defaults: Fixed::default(),
            transfers: 0,
            detectors: 0,
            idles: 0,
        }
    }
    pub fn record_default(
        &mut self,
        result: &Result<zeff_m68k::timing::physical::EntryStep, EntryFault<usize>>,
    ) {
        if self.detector.omitted
            && let Err(EntryFault::TimingUnavailable(CriticalFault::AddressError {
                access,
                direction,
            })) = result
        {
            assert_eq!(*direction, Direction::Read);
            assert_eq!(
                self.detector.report,
                Report {
                    kind: Kind::TimingUnavailable,
                    elapsed: 0
                }
            );
            self.defaults.push(Detection {
                phase: PHASES[self.transfers],
                access: *access,
                start: self.ordinary.clock,
                detector: self.detector,
            });
        }
    }
}

impl ClockedBus for Device {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.ordinary.read_cycle(access, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.ordinary.write_cycle(access, value, start)
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.ordinary.idle(start, clocks);
    }
}

impl EntryBus for Device {
    type Error = usize;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        self.ordinary.valid &= self.ordinary.clock == start.0;
        let index = self.transfers;
        let phase = PHASES[index];
        let report = self.reports.slots[index].unwrap();
        let value = self.ordinary.words.sample(access.bus_address);
        self.events.push(Event::Transfer {
            phase,
            access,
            value,
            start: start.0,
            report,
            index,
        });
        self.scripts.push(Script {
            phase,
            access,
            value,
            report,
            index,
        });
        self.ordinary.clock += u64::from(report.elapsed);
        self.transfers += 1;
        EntryReport {
            elapsed: report.elapsed,
            outcome: report.kind.outcome(value, index),
        }
    }
    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), usize> {
        panic!("unexpected reset write")
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.ordinary.valid &= self.ordinary.clock == start.0;
        self.events.push(Event::Idle {
            phase: if self.idles == 0 {
                "ResetI14"
            } else {
                "ResetI2"
            },
            start: start.0,
            elapsed: clocks,
        });
        self.idles += 1;
        self.ordinary.clock += u64::from(clocks);
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        assert!(!self.detector.omitted);
        assert_eq!(direction, Direction::Read);
        self.ordinary.valid &= self.ordinary.clock == start.0;
        let detection = Detection {
            phase: PHASES[self.transfers],
            access,
            start: start.0,
            detector: self.detector,
        };
        self.defaults.push(detection);
        self.events.push(Event::Detection(detection));
        self.detectors += 1;
        self.ordinary.clock += u64::from(self.detector.report.elapsed);
        EntryReport {
            elapsed: self.detector.report.elapsed,
            outcome: self.detector.report.kind.outcome((), 0),
        }
    }
}

pub struct DefaultDetector<'a>(pub &'a mut Device);
impl EntryBus for DefaultDetector<'_> {
    type Error = usize;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        self.0.read_entry(access, start)
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        self.0.write_entry(access, value, start)
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle_entry(start, clocks);
    }
}
