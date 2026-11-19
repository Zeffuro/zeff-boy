use crate::device::{Datum, Device, Event, Mutation, Script};
use crate::literal::Value;
use crate::oracle::Expected;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{TimedStep, TimingFault};

pub(crate) fn object(fields: Vec<(&'static str, Value)>) -> Value {
    Value::Object(fields)
}
pub(crate) fn n(value: impl Into<u64>) -> Value {
    Value::Integer(value.into())
}
pub(crate) fn s(value: &'static str) -> Value {
    Value::Text(value)
}
pub(crate) fn b(value: bool) -> Value {
    Value::Bool(value)
}
pub(crate) fn opt<T>(value: Option<T>, build: impl FnOnce(T) -> Value) -> Value {
    value.map_or(Value::Null, build)
}
pub(crate) fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(values.into_iter().collect())
}

pub(crate) fn registers(r: &Registers) -> Value {
    object(vec![
        ("d", list(r.d.map(n))),
        ("a", list(r.a.map(n))),
        ("pc", n(r.pc)),
        ("ssp", n(r.ssp)),
        ("usp", n(r.usp)),
        ("sr", n(r.status_register())),
    ])
}

pub(crate) fn queue(words: [u16; 2]) -> Value {
    list(words.map(n))
}

pub(crate) fn cpu(expected: &Expected, initial: bool) -> Value {
    let (r, words, clock, frozen, trace) = if initial {
        (
            &expected.initial.r,
            expected.initial.queue,
            expected.initial.device.clock,
            false,
            false,
        )
    } else {
        (
            &expected.r,
            expected.queue,
            expected.device.clock,
            expected.error.is_some(),
            expected.trace,
        )
    };
    object(vec![
        ("registers", registers(r)),
        ("queue", queue(words)),
        ("clock_u64", Value::Decimal(clock)),
        (
            "private_cpu",
            object(vec![
                (
                    "pipeline",
                    object(vec![("address", n(r.pc)), ("words", queue(words))]),
                ),
                ("frozen", b(frozen)),
                ("stopped", b(false)),
                ("pending_trace", b(trace)),
                (
                    "interrupts",
                    object(vec![
                        ("level", n(expected.initial.level)),
                        ("level7_edge", b(expected.initial.edge)),
                    ]),
                ),
                ("physical_state", object(vec![("Idle", Value::Null)])),
                ("physical_inspection", s("DiagnosticCompleteValues")),
                ("imported_boundary", Value::Null),
            ]),
        ),
    ])
}

fn access(a: BusAccess) -> Value {
    object(vec![
        ("logical_address", n(a.logical_address)),
        ("bus_address", n(a.bus_address)),
        (
            "FC",
            n(match a.function_code {
                FunctionCode::UserData => 1u8,
                FunctionCode::UserProgram => 2,
                FunctionCode::SupervisorData => 5,
                FunctionCode::SupervisorProgram => 6,
                FunctionCode::CpuSpace => 7,
            }),
        ),
        (
            "kind",
            s(match a.kind {
                AccessKind::Data => "Data",
                AccessKind::Prefetch => "Prefetch",
                AccessKind::Stack => "Stack",
                AccessKind::Vector => "Vector",
                AccessKind::InterruptAcknowledge => "InterruptAcknowledge",
                _ => panic!("kind"),
            }),
        ),
        (
            "lanes",
            s(match a.lanes {
                ByteLanes::Upper => "Upper",
                ByteLanes::Lower => "Lower",
                ByteLanes::Both => "Both",
            }),
        ),
    ])
}

fn direction(d: Direction) -> Value {
    s(if d == Direction::Read {
        "Read"
    } else {
        "Write"
    })
}

fn datum(value: Datum) -> Value {
    match value {
        Datum::Word(word) => n(word),
        Datum::Reply(reply) => s(reply.name()),
    }
}

fn event(e: Event) -> Value {
    match e {
        Event::Idle {
            phase,
            start,
            clocks,
        } => object(vec![
            ("phase", s(phase)),
            ("start_clock_u64", Value::Decimal(start)),
            ("elapsed", n(clocks)),
        ]),
        Event::Transfer {
            phase,
            access: a,
            direction: d,
            value,
            start,
            wait,
            elapsed,
            failed,
            deposited,
            index,
        } => object(vec![
            ("phase", s(phase)),
            ("access", access(a)),
            ("direction", direction(d)),
            ("value", datum(value)),
            ("start_clock_u64", Value::Decimal(start)),
            ("wait_clocks", n(wait)),
            ("elapsed", n(elapsed)),
            (
                "outcome",
                s(if failed { "HostFailure" } else { "Complete" }),
            ),
            ("deposited", b(deposited)),
            ("index", n(index as u64)),
        ]),
    }
}

fn script(script: Script) -> Value {
    object(vec![
        ("phase", s(script.phase)),
        ("access", access(script.access)),
        ("direction", direction(script.direction)),
        ("wait_clocks", n(script.wait)),
        (
            "result",
            match script.result {
                Ok(value) => object(vec![("Ok", opt(value, datum))]),
                Err(index) => object(vec![("Err", n(index as u64))]),
            },
        ),
    ])
}

fn mutation(change: Mutation) -> Value {
    object(vec![
        ("phase", s(change.phase)),
        ("timing", s(if change.after { "after" } else { "before" })),
        ("address", n(change.address)),
        ("value", n(change.value)),
    ])
}

fn wait_config(device: &Device) -> Value {
    object(
        device
            .wait_config
            .iter()
            .map(|(name, wait)| (*name, n(*wait)))
            .collect(),
    )
}

fn bus(_expected: &Expected, device: &Device) -> Value {
    let mut words = device.words.iter().copied().collect::<Vec<_>>();
    words.sort_unstable();
    object(vec![
        (
            "word_RAM",
            list(
                words
                    .into_iter()
                    .map(|(address, value)| list([n(address), n(value)])),
            ),
        ),
        ("events", list(device.events.iter().copied().map(event))),
        ("clock_u64", Value::Decimal(device.clock)),
        ("transfers", n(device.transfers as u64)),
        ("waits", wait_config(device)),
        ("fail_phase", opt(device.fail, s)),
        ("failed_write_deposit", b(device.failed_write_deposit)),
    ])
}

pub(crate) fn expanded(expected: &Expected, device: &Device) -> Value {
    let Value::Object(mut fields) = bus(expected, device) else {
        unreachable!()
    };
    fields.extend(vec![
        ("scripts", list(device.scripts.iter().copied().map(script))),
        (
            "phase_config",
            list(
                device.phases[..device.phase_count]
                    .iter()
                    .map(|phase| s(phase.unwrap())),
            ),
        ),
        ("mutation", opt(device.mutation, mutation)),
        (
            "mutation_effects",
            list(device.effects.iter().map(|effect| {
                object(vec![
                    ("phase", s(effect.mutation.phase)),
                    (
                        "timing",
                        s(if effect.mutation.after {
                            "after"
                        } else {
                            "before"
                        }),
                    ),
                    ("logical_address", n(effect.mutation.address)),
                    (
                        "bus_address",
                        n(Device::mutation_bus_address(effect.mutation)),
                    ),
                    ("before", n(effect.before)),
                    ("after", n(effect.mutation.value)),
                    ("clock_u64", Value::Decimal(effect.clock)),
                ])
            })),
        ),
        ("valid", b(device.valid)),
        (
            "fixed_word_slots",
            list(
                device
                    .words
                    .slots
                    .map(|pair| opt(pair, |(address, value)| list([n(address), n(value)]))),
            ),
        ),
        ("word_count", n(device.words.len as u64)),
        (
            "fixed_event_slots",
            list(device.events.slots.map(|slot| opt(slot, event))),
        ),
        ("event_count", n(device.events.len as u64)),
        (
            "fixed_script_slots",
            list(device.scripts.slots.map(|slot| opt(slot, script))),
        ),
        ("script_count", n(device.scripts.len as u64)),
        (
            "fixed_phase_slots",
            list(device.phases.map(|phase| opt(phase, s))),
        ),
        ("fixed_wait_slots", list(device.waits.map(n))),
    ]);
    if device.entry_mode.is_some() {
        fields.extend(vec![
            ("entry_fail_phase", opt(device.entry_fail, s)),
            ("entry_failed_write_deposit", b(device.entry_failed_deposit)),
            ("acknowledge_reply", s(device.reply.name())),
        ]);
    }
    object(fields)
}

pub(crate) fn outcome(result: Result<TimedStep, TimingFault<usize>>) -> Value {
    match result {
        Ok(step) => object(vec![(
            "Ok",
            object(vec![(
                "TimedStep",
                object(vec![
                    ("opcode", n(step.opcode)),
                    ("clocks_u64", Value::Decimal(step.clocks)),
                    ("exception", opt(step.exception, n)),
                ]),
            )]),
        )]),
        Err(TimingFault::ClockOverflow) => object(vec![(
            "Err",
            object(vec![("TimingFault::ClockOverflow", Value::Null)]),
        )]),
        Err(TimingFault::AddressError { address }) => object(vec![(
            "Err",
            object(vec![(
                "TimingFault::AddressError",
                object(vec![("address", n(address))]),
            )]),
        )]),
        Err(TimingFault::InterruptAcknowledgeUnsupported) => object(vec![(
            "Err",
            object(vec![(
                "TimingFault::InterruptAcknowledgeUnsupported",
                Value::Null,
            )]),
        )]),
        Err(TimingFault::Bus {
            access: a,
            direction: d,
            source,
        }) => object(vec![(
            "Err",
            object(vec![(
                "TimingFault::Bus",
                object(vec![
                    ("access", access(a)),
                    ("direction", direction(d)),
                    ("source", n(source as u64)),
                ]),
            )]),
        )]),
        _ => panic!("outcome"),
    }
}

pub fn wrapper(expected: &Expected, axis: Value) -> Value {
    let seed = &expected.initial;
    let requested_phase = seed
        .axis
        .find("reserve_phase")
        .or_else(|| seed.axis.find("fail_phase"))
        .map(Value::text);
    let calculation_published = expected.images.iter().any(|image| image.phase == "F");
    object(vec![
        ("axis", axis),
        (
            "initial",
            object(vec![
                (
                    "constructor_input",
                    object(vec![
                        ("registers", registers(&seed.r)),
                        ("words", queue(seed.queue)),
                        ("clock_u64", Value::Decimal(seed.device.clock)),
                    ]),
                ),
                (
                    "CPU",
                    if expected.importable {
                        cpu(expected, true)
                    } else {
                        Value::Null
                    },
                ),
                ("device", expanded(expected, &seed.device)),
            ]),
        ),
        (
            "expected",
            if expected.importable {
                object(vec![
                    ("typed_result", outcome(expected.result())),
                    ("CPU", cpu(expected, false)),
                    ("device", bus(expected, &expected.device)),
                    (
                        "phase_register_images",
                        list(expected.images.iter().map(|image| {
                            object(vec![
                                ("phase", s(image.phase)),
                                ("registers", registers(&image.registers)),
                            ])
                        })),
                    ),
                    ("latched_source", opt(expected.latched, n)),
                    (
                        "calculated_result",
                        opt(
                            if calculation_published {
                                expected.calculated
                            } else {
                                None
                            },
                            n,
                        ),
                    ),
                ])
            } else {
                Value::Null
            },
        ),
        ("expected_device", expanded(expected, &expected.device)),
        (
            "geometry",
            object(vec![
                ("requested_EA", opt(seed.requested, n)),
                ("seed_operand_EA", opt(seed.operand_ea, n)),
                ("initial_actual_EA", opt(seed.initial_ea, n)),
                ("cached_EA", opt(expected.cached, n)),
                ("import_admissible", b(expected.importable)),
                (
                    "initial_bank",
                    s(if seed.r.status_register() & 0x2000 != 0 {
                        "supervisor"
                    } else {
                        "user"
                    }),
                ),
            ]),
        ),
        (
            "reachability",
            object(vec![
                (
                    "consumed_callbacks",
                    list(expected.device.scripts.iter().map(|script| s(script.phase))),
                ),
                (
                    "reserve_attempts",
                    list(expected.attempts.iter().map(|phase| s(phase))),
                ),
                ("fault_phase", opt(expected.fault_phase, s)),
                ("blocked_phase", opt(expected.blocked_phase, s)),
                ("requested_phase", opt(requested_phase, s)),
                (
                    "constructor",
                    s(if expected.importable { "Some" } else { "None" }),
                ),
            ]),
        ),
    ])
}
