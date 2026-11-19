use crate::fixed::Fixed;
use crate::literal::Value;
use crate::oracle::{self, Expected as Primary};
use crate::reset_device::{Detection, Device, Event, Kind, PHASES, Script};
use crate::seed::{self, Axis};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryFault, EntryInputError, EntryKind, EntryStep, ExternalResetBoundary,
    PhysicalEntryState, PhysicalInspection, ResetQualification, TimingQualification,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
    pub frozen: bool,
    pub trace: bool,
    pub stopped: bool,
    pub level: u8,
    pub edge: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
}
impl Cpu {
    pub fn origin(primary: &Primary) -> Self {
        Self {
            r: primary.r,
            queue: Some(primary.queue),
            clock: primary.device.clock,
            frozen: primary.error.is_some(),
            trace: primary.trace,
            stopped: false,
            level: primary.initial.level,
            edge: primary.initial.edge,
            state: PhysicalEntryState::Idle,
            inspection: PhysicalInspection::DiagnosticCompleteValues,
        }
    }
    pub fn debug(&self) -> String {
        let pipeline = self.queue.map_or_else(
            || "None".to_owned(),
            |words| {
                format!(
                    "Some(Pipeline {{ address: {}, words: {:?} }})",
                    self.r.pc, words
                )
            },
        );
        format!(
            "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: {}, frozen: {}, stopped: {}, interrupts: InterruptState {{ level: {}, level7_edge: {} }}, pending_trace: {}, physical_state: {:?}, physical_inspection: {:?}, imported_boundary: None }}",
            self.r,
            self.clock,
            pipeline,
            self.frozen,
            self.stopped,
            self.level,
            self.edge,
            self.trace,
            self.state,
            self.inspection
        )
    }
    pub fn matches(&self, actual: &PrefetchCpu) -> bool {
        actual.registers == self.r
            && actual.queued_words() == self.queue
            && actual.clock().0 == self.clock
            && actual.stopped() == self.stopped
            && actual.trace_pending() == self.trace
            && actual.physical_entry_state() == self.state
            && actual.physical_inspection() == self.inspection
            && format!("{actual:?}") == self.debug()
    }
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub axis: Value,
    pub request: Value,
    pub origin: Primary,
    pub initial_device: Device,
    pub origin_device: Device,
    pub input_cpu: Cpu,
    pub input_device: Device,
    pub cpu: Cpu,
    pub device: Device,
    pub boundary: ExternalResetBoundary,
    pub result: Result<EntryStep, EntryFault<usize>>,
    pub attempts: Fixed<&'static str>,
    pub consumed: Fixed<&'static str>,
    pub terminal: Option<&'static str>,
    pub rejected: bool,
}
impl Expected {
    fn fail(&mut self, error: EntryFault<usize>, phase: &'static str, frozen: bool) {
        self.result = Err(error);
        self.terminal = Some(phase);
        self.cpu.frozen |= frozen;
    }
    fn idle(&mut self, phase: &'static str, elapsed: u32) -> bool {
        let Some(end) = self.cpu.clock.checked_add(u64::from(elapsed)) else {
            self.fail(EntryFault::ClockOverflow, phase, true);
            return false;
        };
        self.device.events.push(Event::Idle {
            phase,
            start: self.cpu.clock,
            elapsed,
        });
        self.device.idles += 1;
        self.cpu.clock = end;
        self.device.ordinary.clock = end;
        true
    }
    fn halt(&mut self, cause: CriticalFault, kind: Kind, phase: &'static str) {
        let qualified = matches!(kind, Kind::Complete | Kind::CriticalAbortReleased);
        self.cpu.state = PhysicalEntryState::DoubleFaultHalted {
            cause,
            timing: if qualified {
                TimingQualification::Qualified
            } else {
                TimingQualification::Unavailable
            },
        };
        self.cpu.inspection = PhysicalInspection::Unavailable;
        self.fail(
            if qualified {
                EntryFault::DoubleFaultHalted(cause)
            } else {
                EntryFault::TimingUnavailable(cause)
            },
            phase,
            false,
        );
    }
    fn read(&mut self, phase: &'static str, address: u32, vector: bool) -> Option<u16> {
        self.attempts.push(phase);
        if self.cpu.clock > u64::MAX - u64::from(u32::MAX) {
            self.fail(EntryFault::ClockOverflow, phase, true);
            return None;
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: FunctionCode::SupervisorProgram,
            kind: if vector {
                AccessKind::ResetVector
            } else {
                AccessKind::Prefetch
            },
            lanes: ByteLanes::Both,
        };
        if address & 1 != 0 {
            let detector = self.device.detector;
            let detection = Detection {
                phase,
                access,
                start: self.cpu.clock,
                detector,
            };
            self.device.defaults.push(detection);
            if !detector.omitted {
                self.device.events.push(Event::Detection(detection));
                self.device.detectors += 1;
            }
            self.cpu.clock += u64::from(detector.report.elapsed);
            self.device.ordinary.clock = self.cpu.clock;
            if detector.report.kind == Kind::HostFailure {
                self.fail(
                    EntryFault::HostFailure {
                        access,
                        direction: Direction::Read,
                        source: 0,
                    },
                    phase,
                    true,
                );
            } else {
                self.halt(
                    CriticalFault::AddressError {
                        access,
                        direction: Direction::Read,
                    },
                    detector.report.kind,
                    phase,
                );
            }
            return None;
        }
        let index = self.device.transfers;
        let report = self.device.reports.slots[index].unwrap();
        let value = self.device.ordinary.words.sample(access.bus_address);
        self.device.events.push(Event::Transfer {
            phase,
            access,
            value,
            start: self.cpu.clock,
            report,
            index,
        });
        self.device.scripts.push(Script {
            phase,
            access,
            value,
            report,
            index,
        });
        self.device.transfers += 1;
        self.consumed.push(phase);
        self.cpu.clock += u64::from(report.elapsed);
        self.device.ordinary.clock = self.cpu.clock;
        match report.kind {
            Kind::Complete if report.elapsed >= 4 => return Some(value),
            Kind::Complete => self.fail(
                EntryFault::InvalidCompleteDuration {
                    elapsed: report.elapsed,
                },
                phase,
                true,
            ),
            Kind::HostFailure => self.fail(
                EntryFault::HostFailure {
                    access,
                    direction: Direction::Read,
                    source: index,
                },
                phase,
                true,
            ),
            other => self.halt(
                CriticalFault::BusAbort {
                    access,
                    direction: Direction::Read,
                },
                other,
                phase,
            ),
        }
        None
    }
    fn execute(&mut self) {
        if self.cpu.frozen {
            self.rejected = true;
            self.fail(EntryFault::HostFrozen, "preflight", false);
            return;
        }
        if self.boundary.qualification == ResetQualification::Unqualified {
            self.rejected = true;
            self.fail(
                EntryFault::Input(EntryInputError::UnqualifiedReset),
                "preflight",
                false,
            );
            return;
        }
        if self.boundary.sampled_release.0 < self.cpu.clock {
            self.rejected = true;
            self.fail(
                EntryFault::Input(EntryInputError::NonmonotonicRelease),
                "preflight",
                false,
            );
            return;
        }
        self.cpu.clock = self.boundary.sampled_release.0;
        self.cpu.queue = None;
        self.cpu.trace = false;
        self.cpu.stopped = false;
        self.cpu.edge = false;
        self.cpu.state = PhysicalEntryState::Critical(EntryKind::ExternalWarmReset);
        if !self.idle("ResetI14", 14) {
            return;
        }
        let mut vector_words = [0u16; 4];
        for (index, word) in vector_words.iter_mut().enumerate() {
            let Some(value) = self.read(PHASES[index], index as u32 * 2, true) else {
                return;
            };
            *word = value;
        }
        let sp = u32::from(vector_words[0]) << 16 | u32::from(vector_words[1]);
        let target = u32::from(vector_words[2]) << 16 | u32::from(vector_words[3]);
        let Some(first) = self.read("HandlerFirst", target, false) else {
            return;
        };
        if !self.idle("ResetI2", 2) {
            return;
        }
        let Some(second) = self.read("HandlerSecond", target.wrapping_add(2), false) else {
            return;
        };
        self.cpu.r.ssp = sp;
        self.cpu.r.pc = target;
        self.cpu
            .r
            .set_status_register(0x2700 | self.cpu.r.status_register() & 31);
        self.cpu.queue = Some([first, second]);
        self.cpu.inspection = PhysicalInspection::DiagnosticCompleteValues;
        self.result = Ok(EntryStep {
            kind: EntryKind::ExternalWarmReset,
            clocks: self.cpu.clock - self.boundary.sampled_release.0,
        });
    }
}

pub fn predict(axis: Value, request: Value) -> Expected {
    let origin = oracle::predict(seed::build(&Axis::from_literal(&axis)));
    assert!(origin.importable);
    let initial_device = Device::new(origin.initial.device.clone(), &request);
    let origin_device = Device::new(origin.device.clone(), &request);
    let mut input_cpu = Cpu::origin(&origin);
    input_cpu.level = request
        .find("level")
        .map_or(input_cpu.level, |v| v.number() as u8);
    input_cpu.edge = request
        .find("level7_edge")
        .map_or(input_cpu.edge, Value::boolean);
    let release = if request
        .find("nonmonotonic_release")
        .is_some_and(Value::boolean)
    {
        input_cpu.clock - 1
    } else {
        request
            .find("release")
            .map_or(input_cpu.clock, Value::number)
    };
    let boundary = ExternalResetBoundary {
        sampled_release: CpuClocks(release),
        qualification: if request.find("qualified").is_some_and(|v| !v.boolean()) {
            ResetQualification::Unqualified
        } else {
            ResetQualification::QualifiedSafeAssertionAndRelease
        },
    };
    let mut input_device = origin_device.clone();
    input_device.ordinary.clock = release;
    let mut expected = Expected {
        axis,
        request,
        origin,
        initial_device,
        origin_device,
        cpu: input_cpu.clone(),
        device: input_device.clone(),
        input_cpu,
        input_device,
        boundary,
        result: Ok(EntryStep {
            kind: EntryKind::ExternalWarmReset,
            clocks: 0,
        }),
        attempts: Fixed::default(),
        consumed: Fixed::default(),
        terminal: None,
        rejected: false,
    };
    expected.execute();
    expected
}
