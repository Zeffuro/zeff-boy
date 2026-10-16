use crate::binding::{self, b, list, n, object, opt, s};
use crate::literal::Value;
use crate::reset_device::{Detection, Device, Event, Kind, Report, Script};
use crate::reset_engine::{Cpu, Expected};
use zeff_m68k::bus::{AccessKind, BusAccess, Direction};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryFault, EntryInputError, EntryStep, PhysicalEntryState, TimingQualification,
};

fn put(value: &mut Value, key: &'static str, item: Value) {
    let Value::Object(fields) = value else {
        panic!("object");
    };
    if let Some((_, old)) = fields.iter_mut().find(|(name, _)| *name == key) {
        *old = item;
    } else {
        fields.push((key, item));
    }
}
fn access(a: BusAccess) -> Value {
    object(vec![
        ("logical_address", n(a.logical_address)),
        ("bus_address", n(a.bus_address)),
        ("FC", n(a.function_code as u8)),
        (
            "kind",
            s(match a.kind {
                AccessKind::ResetVector => "ResetVector",
                AccessKind::Prefetch => "Prefetch",
                _ => panic!("reset access"),
            }),
        ),
        ("lanes", s("Both")),
    ])
}
fn cause(value: CriticalFault) -> Value {
    let (name, a, direction) = match value {
        CriticalFault::AddressError { access, direction } => {
            ("CriticalFault::AddressError", access, direction)
        }
        CriticalFault::BusAbort { access, direction } => {
            ("CriticalFault::BusAbort", access, direction)
        }
    };
    assert_eq!(direction, Direction::Read);
    object(vec![(
        name,
        object(vec![("access", access(a)), ("direction", s("Read"))]),
    )])
}
fn state(value: PhysicalEntryState) -> Value {
    match value {
        PhysicalEntryState::Idle => object(vec![("Idle", Value::Null)]),
        PhysicalEntryState::Critical(_) => object(vec![("Critical", s("ExternalWarmReset"))]),
        PhysicalEntryState::DoubleFaultHalted { cause: c, timing } => object(vec![(
            "DoubleFaultHalted",
            object(vec![
                ("cause", cause(c)),
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
        _ => panic!("reset state"),
    }
}
pub fn cpu(value: &Cpu) -> Value {
    object(vec![("registers", binding::registers(&value.r)), ("queue", opt(value.queue, binding::queue)),
        ("clock_u64", Value::Decimal(value.clock)), ("private_cpu", object(vec![
            ("pipeline", opt(value.queue, |words| object(vec![("address", n(value.r.pc)), ("words", binding::queue(words))]))),
            ("frozen", b(value.frozen)), ("stopped", b(value.stopped)), ("pending_trace", b(value.trace)),
            ("interrupts", object(vec![("level", n(value.level)), ("level7_edge", b(value.edge))])),
            ("physical_state", state(value.state)), ("physical_inspection", s(match value.inspection {
                zeff_m68k::timing::physical::PhysicalInspection::DiagnosticCompleteValues => "DiagnosticCompleteValues",
                zeff_m68k::timing::physical::PhysicalInspection::Unavailable => "Unavailable",
            })), ("imported_boundary", Value::Null),
        ]))])
}
pub fn outcome(value: &Result<EntryStep, EntryFault<usize>>) -> Value {
    let error = match value {
        Ok(step) => {
            return object(vec![(
                "Ok",
                object(vec![(
                    "EntryStep",
                    object(vec![
                        ("kind", s("ExternalWarmReset")),
                        ("clocks_u64", Value::Decimal(step.clocks)),
                    ]),
                )]),
            )]);
        }
        Err(EntryFault::ClockOverflow) => ("EntryFault::ClockOverflow", Value::Null),
        Err(EntryFault::HostFrozen) => ("EntryFault::HostFrozen", Value::Null),
        Err(EntryFault::Input(reason)) => (
            "EntryFault::Input",
            object(vec![(
                match reason {
                    EntryInputError::UnqualifiedReset => "EntryInputError::UnqualifiedReset",
                    EntryInputError::NonmonotonicRelease => "EntryInputError::NonmonotonicRelease",
                    _ => panic!("reset input"),
                },
                Value::Null,
            )]),
        ),
        Err(EntryFault::InvalidCompleteDuration { elapsed }) => (
            "EntryFault::InvalidCompleteDuration",
            object(vec![("elapsed", n(*elapsed))]),
        ),
        Err(EntryFault::HostFailure {
            access: a,
            direction,
            source,
        }) => {
            assert_eq!(*direction, Direction::Read);
            (
                "EntryFault::HostFailure",
                object(vec![
                    ("access", access(*a)),
                    ("direction", s("Read")),
                    ("source", n(*source as u64)),
                ]),
            )
        }
        Err(EntryFault::DoubleFaultHalted(c)) => ("EntryFault::DoubleFaultHalted", cause(*c)),
        Err(EntryFault::TimingUnavailable(c)) => ("EntryFault::TimingUnavailable", cause(*c)),
        _ => panic!("reset outcome"),
    };
    object(vec![("Err", object(vec![error]))])
}
fn report(value: Report) -> Value {
    object(vec![
        ("outcome", s(value.kind.name())),
        ("elapsed", n(value.elapsed)),
    ])
}
fn detection(value: Detection) -> Value {
    object(vec![
        ("phase", s(value.phase)),
        ("access", access(value.access)),
        ("direction", s("Read")),
        ("start_clock_u64", Value::Decimal(value.start)),
        ("elapsed", n(value.detector.report.elapsed)),
        ("outcome", s(value.detector.report.kind.name())),
        ("omitted", b(value.detector.omitted)),
    ])
}
fn event(value: Event) -> Value {
    match value {
        Event::Idle {
            phase,
            start,
            elapsed,
        } => object(vec![
            ("phase", s(phase)),
            ("start_clock_u64", Value::Decimal(start)),
            ("elapsed", n(elapsed)),
        ]),
        Event::Detection(value) => {
            let mut body = detection(value);
            put(&mut body, "value", Value::Null);
            put(&mut body, "detector_only", b(true));
            body
        }
        Event::Transfer {
            phase,
            access: a,
            value,
            start,
            report: r,
            index,
        } => object(vec![
            ("phase", s(phase)),
            ("access", access(a)),
            ("direction", s("Read")),
            ("value", n(value)),
            ("start_clock_u64", Value::Decimal(start)),
            ("elapsed", n(r.elapsed)),
            ("outcome", s(r.kind.name())),
            ("detector_only", b(false)),
            ("index", n(index as u64)),
        ]),
    }
}
fn script(value: Script) -> Value {
    object(vec![
        ("phase", s(value.phase)),
        ("access", access(value.access)),
        ("direction", s("Read")),
        ("elapsed", n(value.report.elapsed)),
        (
            "report",
            object(vec![
                ("outcome", s(value.report.kind.name())),
                (
                    "value",
                    if value.report.kind == Kind::Complete {
                        n(value.value)
                    } else {
                        Value::Null
                    },
                ),
                (
                    "source",
                    if value.report.kind == Kind::HostFailure {
                        n(value.index as u64)
                    } else {
                        Value::Null
                    },
                ),
            ]),
        ),
    ])
}
fn configuration(value: &mut Value, device: &Device) {
    put(
        value,
        "reset_reports",
        list(device.reports.iter().map(|r| report(*r))),
    );
    put(
        value,
        "fixed_reset_report_slots",
        list(device.reports.slots.map(|r| opt(r, report))),
    );
    put(
        value,
        "reset_detector",
        object(vec![
            ("outcome", s(device.detector.report.kind.name())),
            ("elapsed", n(device.detector.report.elapsed)),
            ("omitted", b(device.detector.omitted)),
        ]),
    );
    put(value, "reset_transfers", n(device.transfers as u64));
    put(value, "reset_detectors", n(device.detectors as u64));
    put(
        value,
        "reset_defaults",
        list(device.defaults.iter().map(|d| detection(*d))),
    );
}
pub fn device(expected: &Expected, device: &Device) -> Value {
    let mut body = binding::expanded(&expected.origin, &device.ordinary);
    let mut events = body.get("events").list().to_vec();
    events.extend(device.events.iter().map(|value| event(*value)));
    let mut scripts = body.get("scripts").list().to_vec();
    scripts.extend(device.scripts.iter().map(|value| script(*value)));
    assert!(events.len() <= 48 && scripts.len() <= 48);
    for (name, slots, count, rows) in [
        ("events", "fixed_event_slots", "event_count", events),
        ("scripts", "fixed_script_slots", "script_count", scripts),
    ] {
        put(&mut body, count, n(rows.len() as u64));
        let mut fixed = rows.clone();
        fixed.resize(48, Value::Null);
        put(&mut body, name, Value::List(rows));
        put(&mut body, slots, Value::List(fixed));
    }
    put(
        &mut body,
        "transfers",
        n((device.ordinary.transfers + device.transfers) as u64),
    );
    configuration(&mut body, device);
    body
}
pub fn wrapper(expected: &Expected) -> Value {
    let mut origin = binding::wrapper(&expected.origin, expected.axis.clone());
    let mut initial = origin.get("initial").clone();
    put(
        &mut initial,
        "device",
        device(expected, &expected.initial_device),
    );
    put(&mut origin, "initial", initial.clone());
    put(
        &mut origin,
        "expected_device",
        device(expected, &expected.origin_device),
    );
    let mut origin_expected = origin.get("expected").clone();
    let mut ordinary_device = origin_expected.get("device").clone();
    configuration(&mut ordinary_device, &expected.origin_device);
    put(&mut origin_expected, "device", ordinary_device);
    put(&mut origin, "expected", origin_expected);
    object(vec![
        ("axis", expected.axis.clone()),
        ("reset_request", expected.request.clone()),
        ("initial", initial),
        ("origin", origin),
        (
            "reset_input",
            object(vec![
                ("CPU", cpu(&expected.input_cpu)),
                ("device", device(expected, &expected.input_device)),
            ]),
        ),
        (
            "expected_reset",
            object(vec![
                ("typed_result", outcome(&expected.result)),
                ("CPU", cpu(&expected.cpu)),
                ("device", device(expected, &expected.device)),
            ]),
        ),
        (
            "reset_reachability",
            object(vec![
                ("rejected", b(expected.rejected)),
                (
                    "reserve_attempts",
                    list(expected.attempts.iter().map(|phase| s(phase))),
                ),
                (
                    "consumed_callbacks",
                    list(expected.consumed.iter().map(|phase| s(phase))),
                ),
                ("terminal_phase", opt(expected.terminal, s)),
                (
                    "reset_events",
                    list(expected.device.events.iter().map(|value| event(*value))),
                ),
            ]),
        ),
    ])
}
