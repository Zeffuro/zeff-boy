use crate::binding::{self, b, list, n, object, opt, s};
use crate::device::{Device, Event, ReportKind, Script};
use crate::literal::Value;
use crate::preflight_axes::{EXTRA, TRACE};
use crate::preflight_state::{Constructor, Cpu, Expected, ResultValue};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryFault, ImportedGroup0Boundary, PhysicalEntryState, PhysicalInspection,
    TimingQualification,
};
use zeff_m68k::timing::physical_instruction::InstructionFault;
use zeff_m68k::timing::{TimedOutcome, TimingFault};

fn report(kind: ReportKind, elapsed: u32) -> Value {
    object(vec![("outcome", s(kind.name())), ("elapsed", n(elapsed))])
}
fn detector() -> Value {
    object(vec![
        ("outcome", s("TimingUnavailable")),
        ("elapsed", n(0u8)),
        ("omitted", b(true)),
    ])
}
pub fn config(d: &Device) -> Vec<(&'static str, Value)> {
    if d.reset_reports[0].is_none() {
        return Vec::new();
    }
    let reports = || {
        list(d.reset_reports[..6].iter().map(|slot| {
            let (kind, clocks) = slot.unwrap();
            report(kind, clocks)
        }))
    };
    let slots = || {
        list(
            d.reset_reports
                .iter()
                .map(|slot| opt(*slot, |(kind, clocks)| report(kind, clocks))),
        )
    };
    vec![
        ("reset_reports", reports()),
        ("fixed_reset_report_slots", slots()),
        ("reset_detector", detector()),
        ("reset_transfers", n(d.reset_transfers as u64)),
        ("reset_detectors", n(d.reset_detectors as u64)),
        ("reset_defaults", list([])),
        (
            "preflight_config",
            object(vec![
                ("state", s(d.state.name())),
                ("stage", s(if d.probe { "probe" } else { "setup" })),
                ("probe_transfers", n(d.probe_transfers as u64)),
                ("probe_idles", n(d.probe_idles as u64)),
                (
                    "ordinary_roles",
                    list(
                        d.phases[..d.phase_count]
                            .iter()
                            .filter_map(|phase| phase.filter(|name| !EXTRA.contains(name)))
                            .map(s),
                    ),
                ),
                (
                    "probe_prime_roles",
                    list([s("ProbeFetch0"), s("ProbeFetch1")]),
                ),
                ("probe_trace_roles", list(TRACE.map(s))),
                ("physical_report", report(ReportKind::TimingUnavailable, 5)),
                (
                    "fixed_physical_report_slots",
                    list(
                        d.physical_reports
                            .iter()
                            .map(|slot| opt(*slot, |(kind, clocks)| report(kind, clocks))),
                    ),
                ),
                ("reset_reports", reports()),
                ("fixed_reset_report_slots", slots()),
                ("reset_detector", detector()),
                (
                    "phase_routing",
                    s(
                        "Public setup stage; probe Prefetch without trace-prefix idle selects Prime roles; trace-prefix idle selects short-frame roles",
                    ),
                ),
            ]),
        ),
    ]
}

pub fn physical_event(e: Event) -> Value {
    let Event::Physical {
        phase,
        access,
        start,
        elapsed,
        value,
        outcome,
        instruction,
        index,
    } = e
    else {
        panic!("physical event");
    };
    let mut fields = vec![
        ("phase", s(phase)),
        ("access", binding::access(access)),
        ("direction", s("Read")),
        ("value", n(value)),
        ("start_clock_u64", Value::Decimal(start)),
        ("elapsed", n(elapsed)),
        ("outcome", s(outcome.name())),
        ("index", n(index as u64)),
    ];
    if instruction {
        fields.push(("protocol", s("InstructionBus")));
    } else {
        fields.push(("detector_only", b(false)));
    }
    object(fields)
}
pub fn physical_script(script: Script) -> Value {
    let Script::Physical {
        phase,
        access,
        elapsed,
        outcome,
        instruction,
    } = script
    else {
        panic!("physical script");
    };
    let mut fields = vec![
        ("phase", s(phase)),
        ("access", binding::access(access)),
        ("direction", s("Read")),
        ("elapsed", n(elapsed)),
        (
            "report",
            object(vec![
                ("outcome", s(outcome.name())),
                ("value", Value::Null),
                ("source", Value::Null),
            ]),
        ),
    ];
    if instruction {
        fields.push(("protocol", s("InstructionBus")));
    }
    object(fields)
}

fn cause(value: CriticalFault) -> Value {
    let (name, access, direction) = match value {
        CriticalFault::BusAbort { access, direction } => {
            ("CriticalFault::BusAbort", access, direction)
        }
        CriticalFault::AddressError { access, direction } => {
            ("CriticalFault::AddressError", access, direction)
        }
    };
    object(vec![(
        name,
        object(vec![
            ("access", binding::access(access)),
            ("direction", binding::direction(direction)),
        ]),
    )])
}

pub fn boundary(value: ImportedGroup0Boundary) -> Value {
    let capture = value.capture;
    object(vec![
        ("registers", binding::registers(&value.registers)),
        ("clock_u64", Value::Decimal(value.clock.0)),
        (
            "capture",
            object(vec![
                ("vector", n(capture.vector)),
                ("internal_pc", n(capture.internal_pc)),
                ("phase_sr", n(capture.phase_sr)),
                ("ird", n(capture.ird)),
                ("logical_address", n(capture.logical_address)),
                ("function_code", n(capture.function_code as u8)),
                ("direction", binding::direction(capture.direction)),
                ("processing_class", s("InstructionOrGroup2")),
                ("ssw_profile", s("SelectedIrdUpperBits")),
            ]),
        ),
    ])
}

pub fn cpu(value: &Cpu) -> Value {
    let physical = match value.physical {
        PhysicalEntryState::Idle => object(vec![("Idle", Value::Null)]),
        PhysicalEntryState::Critical(_) => object(vec![("Critical", s("ExternalWarmReset"))]),
        PhysicalEntryState::FaultUnqualified { cause: fault } => object(vec![(
            "FaultUnqualified",
            object(vec![("cause", cause(fault))]),
        )]),
        PhysicalEntryState::DoubleFaultHalted {
            cause: fault,
            timing,
        } => object(vec![(
            "DoubleFaultHalted",
            object(vec![
                ("cause", cause(fault)),
                (
                    "timing",
                    s(if timing == TimingQualification::Qualified {
                        "Qualified"
                    } else {
                        "Unavailable"
                    }),
                ),
            ]),
        )]),
    };
    object(vec![
        ("registers", binding::registers(&value.r)),
        ("clock_u64", Value::Decimal(value.clock)),
        ("queue", opt(value.queue, binding::queue)),
        (
            "private_cpu",
            object(vec![
                (
                    "pipeline",
                    opt(value.queue, |words| {
                        object(vec![
                            ("address", n(value.pipeline_address.unwrap())),
                            ("words", binding::queue(words)),
                        ])
                    }),
                ),
                ("frozen", b(value.frozen)),
                ("stopped", b(value.stopped)),
                ("pending_trace", b(value.trace)),
                (
                    "interrupts",
                    object(vec![("level", n(0u8)), ("level7_edge", b(false))]),
                ),
                ("physical_state", physical),
                (
                    "physical_inspection",
                    s(if value.inspection == PhysicalInspection::Unavailable {
                        "Unavailable"
                    } else {
                        "DiagnosticCompleteValues"
                    }),
                ),
                ("imported_boundary", opt(value.imported, boundary)),
            ]),
        ),
    ])
}

fn fault(error: TimingFault<usize>) -> Value {
    let (name, detail) = match error {
        TimingFault::Bus { .. } | TimingFault::ClockOverflow => {
            return binding::outcome(Err(error));
        }
        TimingFault::AddressError { address } => (
            "TimingFault::AddressError",
            object(vec![("address", n(address))]),
        ),
        TimingFault::Frozen => ("TimingFault::Frozen", Value::Null),
        TimingFault::Stopped => ("TimingFault::Stopped", Value::Null),
        TimingFault::PendingTrace => ("TimingFault::PendingTrace", Value::Null),
        TimingFault::Unprimed => ("TimingFault::Unprimed", Value::Null),
        TimingFault::ImportedBoundaryPending => {
            ("TimingFault::ImportedBoundaryPending", Value::Null)
        }
        TimingFault::PhysicalFaultUnqualified => {
            ("TimingFault::PhysicalFaultUnqualified", Value::Null)
        }
        TimingFault::DoubleFaultHalted => ("TimingFault::DoubleFaultHalted", Value::Null),
        _ => panic!("unexpected fault"),
    };
    object(vec![("Err", object(vec![(name, detail)]))])
}

fn typed(value: &ResultValue) -> Value {
    match value {
        ResultValue::Unit | ResultValue::Prime(Ok(())) | ResultValue::Trace(Ok(None)) => {
            object(vec![("Ok", Value::Null)])
        }
        ResultValue::Step(result) => match result {
            Ok(step) => binding::outcome(Ok(*step)),
            Err(error) => fault(error.clone()),
        },
        ResultValue::Prime(Err(error))
        | ResultValue::Trace(Err(error))
        | ResultValue::Boundary(Err(error)) => fault(error.clone()),
        ResultValue::Trace(Ok(Some(clocks))) => object(vec![(
            "Ok",
            object(vec![("SomeClocks", Value::Decimal(*clocks))]),
        )]),
        ResultValue::Boundary(Ok(TimedOutcome::Stopped)) => object(vec![(
            "Ok",
            object(vec![("TimedOutcome::Stopped", Value::Null)]),
        )]),
        ResultValue::Boundary(Ok(TimedOutcome::Trace { clocks })) => object(vec![(
            "Ok",
            object(vec![(
                "TimedOutcome::Trace",
                object(vec![("clocks_u64", Value::Decimal(*clocks))]),
            )]),
        )]),
        ResultValue::Physical(Err(InstructionFault::TransitionTimingUnavailable {
            phase,
            cause: fault,
        })) => {
            let phase = match phase {
                zeff_m68k::timing::physical_instruction::InstructionPhase::OperandRefill => {
                    "OperandRefill"
                }
                zeff_m68k::timing::physical_instruction::InstructionPhase::ExtensionFirst => {
                    "ExtensionFirst"
                }
                _ => "OperandReadHigh",
            };
            object(vec![(
                "Err",
                object(vec![(
                    "InstructionFault::TransitionTimingUnavailable",
                    object(vec![("phase", s(phase)), ("cause", cause(*fault))]),
                )]),
            )])
        }
        ResultValue::Reset(Err(EntryFault::DoubleFaultHalted(fault))) => object(vec![(
            "Err",
            object(vec![("EntryFault::DoubleFaultHalted", cause(*fault))]),
        )]),
        _ => panic!("unexpected typed result"),
    }
}

fn constructor(value: &Constructor) -> Value {
    match value {
        Constructor::Prefetched { r, words, clock } => object(vec![
            ("method", s("PrefetchCpu::from_prefetched")),
            ("registers", binding::registers(r)),
            ("words", binding::queue(*words)),
            ("clock_u64", Value::Decimal(*clock)),
            ("result", s("Some")),
        ]),
        Constructor::Default => object(vec![
            ("method", s("PrefetchCpu::default")),
            ("registers", Value::Null),
            ("words", Value::Null),
            ("clock_u64", s("0")),
            ("result", s("Self")),
        ]),
        Constructor::Imported(value) => object(vec![
            ("method", s("PrefetchCpu::from_group0_boundary")),
            ("boundary", boundary(*value)),
            ("result", s("Ok(Self)")),
        ]),
    }
}

pub fn wrapper(expected: &Expected) -> Value {
    let source = binding::wrapper(
        &expected.source,
        expected.axis.primary.source.clone().unwrap(),
    );
    object(vec![
        ("axis", expected.axis.value.clone()),
        (
            "source_seed",
            object(vec![
                ("initial", source.get("initial").clone()),
                ("geometry", source.get("geometry").clone()),
            ]),
        ),
        ("constructor", constructor(&expected.constructor)),
        ("constructor_CPU", cpu(&expected.constructor_cpu)),
        (
            "setup_trace",
            list(expected.records.iter().map(|record| {
                object(vec![
                    ("method", s(record.method)),
                    ("arguments", record.arguments.clone()),
                    ("typed_result", typed(&record.result)),
                    ("CPU", cpu(&record.cpu)),
                    (
                        "device",
                        binding::expanded(&expected.source, &record.device),
                    ),
                ])
            })),
        ),
        (
            "source_setup_phase_register_images",
            list(expected.images.iter().map(|image| {
                object(vec![
                    ("phase", s(image.phase)),
                    ("registers", binding::registers(&image.registers)),
                ])
            })),
        ),
        (
            "probe_input",
            object(vec![
                ("CPU", cpu(&expected.input_cpu)),
                (
                    "device",
                    binding::expanded(&expected.source, &expected.input_device),
                ),
            ]),
        ),
        (
            "expected_probe",
            object(vec![
                ("typed_result", typed(&expected.result)),
                ("CPU", cpu(&expected.cpu)),
                (
                    "device",
                    binding::expanded(&expected.source, &expected.device),
                ),
            ]),
        ),
        (
            "reachability",
            object(vec![
                ("execution_guard", opt(expected.guard, s)),
                (
                    "probe_events",
                    list(
                        expected
                            .device
                            .events
                            .iter()
                            .skip(expected.input_device.events.len)
                            .copied()
                            .map(binding::event),
                    ),
                ),
                (
                    "consumed_callbacks",
                    list(
                        expected
                            .device
                            .scripts
                            .iter()
                            .skip(expected.input_device.scripts.len)
                            .copied()
                            .map(|script| s(script.phase())),
                    ),
                ),
                (
                    "probe_source_register_images",
                    s(
                        "Only setup primary phase images are source equations; callback-private CPU reads are not claimed.",
                    ),
                ),
            ]),
        ),
        (
            "input_ownership",
            s(
                "Each API receives independent owned CPU/device copies of the identical per-context/per-state setup snapshot.",
            ),
        ),
    ])
}
