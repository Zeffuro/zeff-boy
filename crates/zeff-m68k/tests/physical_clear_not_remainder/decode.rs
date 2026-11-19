use super::*;
use model::{Case, Event, Image, Outcome, Step};
use value::Value;

fn registers(node: &Value) -> Registers {
    let mut r = Registers::new();
    r.d = node
        .get("d")
        .list()
        .iter()
        .map(Value::u32)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    r.a = node
        .get("a")
        .list()
        .iter()
        .map(Value::u32)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    r.usp = node.get("usp").u32();
    r.ssp = node.get("ssp").u32();
    let sr = node.get("sr").u16();
    r.set_status_register(sr);
    assert_eq!(r.status_register(), sr);
    r.pc = node.get("pc").u32();
    r
}
fn image(node: &Value) -> Image {
    Image {
        registers: registers(node.get("registers")),
        queue: node.get("queue").optional().map(|words| {
            words
                .list()
                .iter()
                .map(Value::u16)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        }),
        clock: node.get("clock_u64").number(),
    }
}
pub(super) fn words(node: &Value) -> Vec<(u32, u16)> {
    node.list()
        .iter()
        .map(|pair| {
            assert_eq!(pair.list().len(), 2);
            (pair.list()[0].u32(), pair.list()[1].u16())
        })
        .collect()
}
fn fc(node: &Value) -> FunctionCode {
    match node.number() {
        1 => FunctionCode::UserData,
        2 => FunctionCode::UserProgram,
        5 => FunctionCode::SupervisorData,
        6 => FunctionCode::SupervisorProgram,
        _ => panic!("literal FC"),
    }
}
fn access(node: &Value) -> BusAccess {
    BusAccess {
        logical_address: node.get("logical_address").u32(),
        bus_address: node.get("bus_address").u32(),
        function_code: fc(node.get("FC")),
        kind: match node.get("kind").text() {
            "Data" => AccessKind::Data,
            "Prefetch" => AccessKind::Prefetch,
            "Stack" => AccessKind::Stack,
            "Vector" => AccessKind::Vector,
            _ => panic!("literal purpose"),
        },
        lanes: match node.get("lanes").text() {
            "both" => ByteLanes::Both,
            "Upper" => ByteLanes::Upper,
            "Lower" => ByteLanes::Lower,
            _ => panic!("literal lanes"),
        },
    }
}
fn direction(node: &Value) -> Direction {
    match node.text() {
        "read" => Direction::Read,
        "write" => Direction::Write,
        _ => panic!("literal direction"),
    }
}
pub(super) fn outcome(node: &Value) -> Outcome {
    match node.text() {
        "Complete" => Outcome::Complete,
        "HostFailure" => Outcome::Host,
        "AtGroup0Entry" => Outcome::Boundary,
        "TimingUnavailable" => Outcome::Unavailable,
        "CriticalAbortReleased" => Outcome::Critical,
        _ => panic!("literal disposition"),
    }
}
pub(super) fn events(node: &Value) -> Vec<Event> {
    node.list()
        .iter()
        .map(|event| {
            if event.get("phase").text().contains("I2") || event.get("phase").text() == "EntryI4" {
                return Event::Idle(
                    event.get("start_clock").number(),
                    event.get("elapsed").u32(),
                );
            }
            let stage = u8::from(event.get("stage").text() == "entry") * 2
                + u8::from(event.get("detector_only").flag());
            Event::Transfer {
                stage,
                access: access(event),
                direction: direction(event.get("direction")),
                value: event.get("value").optional().map(Value::u16),
                start: event.get("start_clock").number(),
                elapsed: event.get("elapsed").u32(),
                outcome: outcome(event.get("outcome")),
                deposited: event.get("word_deposited").flag(),
            }
        })
        .collect()
}
fn cause(node: &Value) -> CriticalFault {
    let (tag, details) = node.single();
    let access = access(details.get("access"));
    let direction = direction(details.get("direction"));
    match tag {
        "CriticalFault::AddressError" => CriticalFault::AddressError { access, direction },
        "CriticalFault::BusAbort" => CriticalFault::BusAbort { access, direction },
        _ => panic!("literal cause"),
    }
}
fn capture(node: &Value) -> (Group0Capture, u16) {
    assert_eq!(node.get("processing_class").text(), "InstructionOrGroup2");
    assert_eq!(node.get("ssw_profile").text(), "SelectedIrdUpperBits");
    (
        Group0Capture {
            vector: node.get("vector").number().try_into().unwrap(),
            internal_pc: node.get("PC").u32(),
            phase_sr: node.get("SR").u16(),
            ird: node.get("IRD").u16(),
            logical_address: node.get("EA").u32(),
            function_code: fc(node.get("FC")),
            direction: direction(node.get("direction")),
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        node.get("SSW").u16(),
    )
}
fn phase(node: &Value) -> InstructionPhase {
    match node.text() {
        "ExtensionFirst" => InstructionPhase::ExtensionFirst,
        "ExtensionSecond" => InstructionPhase::ExtensionSecond,
        "OperandReadHigh" => InstructionPhase::OperandReadHigh,
        "OperandRefill" => InstructionPhase::OperandRefill,
        "OperandWriteHigh" => InstructionPhase::OperandWriteHigh,
        _ => panic!("literal phase"),
    }
}
fn entry_fault(node: &Value) -> EntryFault<&'static str> {
    if let Value::Text("EntryFault::ClockOverflow") = node {
        return EntryFault::ClockOverflow;
    }
    let (tag, details) = node.single();
    match tag {
        "EntryFault::HostFailure" => {
            assert_eq!(details.get("source").text(), "host");
            EntryFault::HostFailure {
                access: access(details.get("access")),
                direction: direction(details.get("direction")),
                source: "host",
            }
        }
        "EntryFault::InvalidCompleteDuration" => EntryFault::InvalidCompleteDuration {
            elapsed: details.get("elapsed").u32(),
        },
        "EntryFault::DoubleFaultHalted" => EntryFault::DoubleFaultHalted(cause(details)),
        "EntryFault::TimingUnavailable" => EntryFault::TimingUnavailable(cause(details)),
        _ => panic!("literal entry fault"),
    }
}
fn result(node: &Value) -> Step {
    let (tag, details) = node.single();
    let (kind, fields) = details.single();
    if tag == "Ok" {
        return Ok(match kind {
            "PhysicalInstructionStep::Instruction" => {
                assert!(fields.get("exception").optional().is_none());
                PhysicalInstructionStep::Instruction(TimedStep {
                    opcode: fields.get("opcode").u16(),
                    clocks: fields.get("clocks_u64").number(),
                    exception: None,
                })
            }
            "PhysicalInstructionStep::Group0" => PhysicalInstructionStep::Group0 {
                opcode: fields.get("opcode").u16(),
                capture: capture(fields.get("capture")).0,
                clocks: fields.get("clocks_u64").number(),
            },
            _ => panic!("literal completed result"),
        });
    }
    assert_eq!(tag, "Err");
    Err(match kind {
        "InstructionFault::Entry" => InstructionFault::Entry(entry_fault(fields)),
        "InstructionFault::TransitionTimingUnavailable" => {
            InstructionFault::TransitionTimingUnavailable {
                phase: phase(fields.get("phase")),
                cause: cause(fields.get("cause")),
            }
        }
        "InstructionFault::CaptureUnavailable" => InstructionFault::CaptureUnavailable {
            phase: phase(fields.get("phase")),
            cause: cause(fields.get("cause")),
        },
        "InstructionFault::TraceUnqualified" => InstructionFault::TraceUnqualified,
        "InstructionFault::InterruptUnqualified" => InstructionFault::InterruptUnqualified,
        "InstructionFault::Execution" => {
            let (name, payload) = fields.single();
            assert_eq!(name, "TimingFault::UnsupportedInstruction");
            InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                pc: payload.get("pc").u32(),
                opcode: payload.get("opcode").u16(),
            })
        }
        _ => panic!("literal fault"),
    })
}
fn state(node: &Value) -> PhysicalEntryState {
    let (name, details) = node.single();
    match name {
        "Idle" => PhysicalEntryState::Idle,
        "Critical" => {
            assert_eq!(details.text(), "ImportedGroup0");
            PhysicalEntryState::Critical(EntryKind::ImportedGroup0)
        }
        "FaultUnqualified" => PhysicalEntryState::FaultUnqualified {
            cause: cause(details.get("cause")),
        },
        "DoubleFaultHalted" => PhysicalEntryState::DoubleFaultHalted {
            cause: cause(details.get("cause")),
            timing: match details.get("timing").text() {
                "Qualified" => TimingQualification::Qualified,
                "Unavailable" => TimingQualification::Unavailable,
                _ => panic!("literal halt timing"),
            },
        },
        _ => panic!("literal physical state"),
    }
}
fn retry(node: &Value) -> Option<TimingFault<&'static str>> {
    node.optional().map(|guard| match guard.text() {
        "Frozen" => TimingFault::Frozen,
        "PhysicalFaultUnqualified" => TimingFault::PhysicalFaultUnqualified,
        "DoubleFaultHalted" => TimingFault::DoubleFaultHalted,
        _ => panic!("literal retry"),
    })
}

fn case(raw: Value) -> Case {
    let e = raw.get("expected");
    let p = e.get("private_cpu");
    let input = raw.get("initial");
    let event_nodes = e.get("events").list();
    let counts = e.get("callback_counts");
    let initial_level = input.get("interrupt_level").number().try_into().unwrap();
    let mode = raw.get("mode").number();
    let extensions = if mode < 5 {
        0
    } else if mode == 7 && raw.get("source_register").number() == 1 {
        2
    } else {
        1
    };
    let mutations = raw
        .get("mutations")
        .list()
        .iter()
        .map(|m| {
            let index = match m.get("phase").text() {
                "E" | "E1" => 0,
                "E2" => 1,
                "RH" => extensions,
                "F" => {
                    if mode == 0 {
                        0
                    } else {
                        extensions + 1
                    }
                }
                _ => panic!("literal mutation phase"),
            };
            (index, m.get("address").u32(), m.get("value").u16())
        })
        .collect();
    Case {
        label: raw.get("id").text(),
        initial: image(input),
        expected: image(e),
        memory: words(input.get("word_RAM")),
        final_memory: words(e.get("word_RAM")),
        events: events(e.get("events")),
        result: result(e.get("typed_result")),
        state: state(p.get("physical_state")),
        inspection: match p.get("physical_inspection").text() {
            "Unavailable" => PhysicalInspection::Unavailable,
            "DiagnosticCompleteValues" => PhysicalInspection::DiagnosticCompleteValues,
            _ => panic!("literal inspection"),
        },
        retry: retry(e.get("retry_guard")),
        frozen: p.get("frozen").flag(),
        queue_address: e.get("queue_address").optional().map(Value::u32),
        capture: e.get("capture").optional().map(capture),
        capture_observable: e.get("capture_observable_from_result").flag(),
        frame: e
            .get("frame_write_order")
            .list()
            .iter()
            .map(|f| (f.get("logical_address").u32(), f.get("word").u16()))
            .collect(),
        before_entry: e.get("before_imported_entry").optional().map(|b| {
            (
                b.get("clock_u64").number(),
                words(b.get("word_RAM")).into_iter().collect(),
            )
        }),
        entry_event: event_nodes
            .iter()
            .position(|node| node.get("phase").text() == "EntryI4"),
        counts: [
            "instruction_transfers",
            "instruction_detectors",
            "entry_transfers",
            "entry_detectors",
            "idles",
        ]
        .map(|key| counts.get(key).number().try_into().unwrap()),
        mutations,
        elapsed: e.get("instruction_elapsed_u64").number(),
        initial_level,
        initial_edge: input.get("level7_edge").flag(),
        expected_level: p
            .get("interrupts")
            .get("level")
            .number()
            .try_into()
            .unwrap(),
        expected_edge: p.get("interrupts").get("level7_edge").flag(),
        default_detector: raw.get("injection").get("default_entry_detector").flag(),
        next_rejection: None,
        raw,
    }
}

pub(super) fn cases() -> Vec<Case> {
    value::records().into_iter().map(case).collect()
}
