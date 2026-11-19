use super::device::Device;
use super::fixed::{Event, Ram};
use super::literal::Value;
use super::model::{Case, Failure, Profile, Report};
use super::oracle::Expected;
use super::{
    AccessKind, BusAccess, ByteLanes, CriticalFault, Direction, FunctionCode, InstructionPhase,
    PhysicalEntryState, PhysicalInspection, Registers, Tally, TimingQualification,
};
mod bridge;
mod preflight;
mod resets;

pub(super) fn registers(node: &Value) -> Registers {
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| node.get("d").list()[i].u32());
    r.a = std::array::from_fn(|i| node.get("a").list()[i].u32());
    r.usp = node.get("usp").u32();
    r.ssp = node.get("ssp").u32();
    r.pc = node.get("pc").u32();
    r.set_status_register(node.get("sr").u16());
    r
}
pub(super) fn ram(node: &Value) -> Ram {
    let mut words = Ram::default();
    for pair in node.list() {
        words.insert(pair.list()[0].u32(), pair.list()[1].u16());
    }
    words
}
pub(super) fn queue(node: &Value) -> Option<[u16; 2]> {
    node.optional()
        .map(|words| [words.list()[0].u16(), words.list()[1].u16()])
}
pub(super) fn access(node: &Value) -> BusAccess {
    BusAccess {
        logical_address: node.get("logical_address").u32(),
        bus_address: node.get("bus_address").u32(),
        function_code: match node.get("FC").u16() {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("literal FC"),
        },
        kind: match node.get("kind").text() {
            "Prefetch" => AccessKind::Prefetch,
            "Data" => AccessKind::Data,
            "Vector" => AccessKind::Vector,
            "ResetVector" => AccessKind::ResetVector,
            "Stack" => AccessKind::Stack,
            _ => panic!("literal kind"),
        },
        lanes: match node.get("lanes").text() {
            "both" => ByteLanes::Both,
            "upper" => ByteLanes::Upper,
            "lower" => ByteLanes::Lower,
            _ => panic!("literal lanes"),
        },
    }
}
pub(super) fn kind(node: &Value) -> Failure {
    match node.text() {
        "Complete" => Failure::Complete,
        "HostFailure" => Failure::Host,
        "AtGroup0Entry" | "CriticalAbortReleased" => Failure::Boundary,
        "TimingUnavailable" => Failure::Unavailable,
        _ => panic!("literal outcome"),
    }
}
pub(super) fn phase(node: &Value) -> InstructionPhase {
    match node.text() {
        "ImmediateFirst" => InstructionPhase::ImmediateFirst,
        "ImmediateSecond" => InstructionPhase::ImmediateSecond,
        "ExtensionFirst" => InstructionPhase::ExtensionFirst,
        "ExtensionSecond" => InstructionPhase::ExtensionSecond,
        "OperandReadHigh" => InstructionPhase::OperandReadHigh,
        "OperandRefill" => InstructionPhase::OperandRefill,
        _ => panic!("literal primary phase"),
    }
}
pub(super) fn event(node: &Value) -> Event {
    if matches!(
        node.get("phase").text(),
        "SourceI2" | "TailI2" | "EntryI4" | "EntryI2" | "ResetI14" | "ResetI2"
    ) {
        return Event::Idle(node.get("start_clock").number(), node.get("elapsed").u32());
    }
    let entry = node.get("stage").text() == "entry";
    let detector = node.get("detector_only").flag();
    Event::Transfer {
        stage: u8::from(entry) * 2 + u8::from(detector),
        phase: if entry {
            None
        } else {
            Some(phase(node.get("phase")))
        },
        access: access(node),
        direction: if node.get("direction").text() == "read" {
            Direction::Read
        } else {
            Direction::Write
        },
        value: node.get("value").optional().map(Value::u16),
        start: node.get("start_clock").number(),
        elapsed: node.get("elapsed").u32(),
        kind: kind(node.get("outcome")),
        deposited: node.get("word_deposited").flag(),
    }
}
pub(super) fn cause(node: &Value) -> CriticalFault {
    let (name, fields) = node.single();
    let a = access(fields.get("access"));
    let direction = if fields.get("direction").text() == "read" {
        Direction::Read
    } else {
        Direction::Write
    };
    match name {
        "CriticalFault::BusAbort" => CriticalFault::BusAbort {
            access: a,
            direction,
        },
        "CriticalFault::AddressError" => CriticalFault::AddressError {
            access: a,
            direction,
        },
        _ => panic!("literal cause"),
    }
}
fn state(node: &Value) -> PhysicalEntryState {
    let (name, fields) = node.single();
    match name {
        "Idle" => PhysicalEntryState::Idle,
        "Critical" => PhysicalEntryState::Critical(match fields.text() {
            "ImportedGroup0" => super::EntryKind::ImportedGroup0,
            "ExternalWarmReset" => super::EntryKind::ExternalWarmReset,
            _ => panic!("literal critical kind"),
        }),
        "FaultUnqualified" => PhysicalEntryState::FaultUnqualified {
            cause: cause(fields.get("cause")),
        },
        "DoubleFaultHalted" => PhysicalEntryState::DoubleFaultHalted {
            cause: cause(fields.get("cause")),
            timing: if fields.get("timing").text() == "Qualified" {
                TimingQualification::Qualified
            } else {
                TimingQualification::Unavailable
            },
        },
        _ => panic!("literal state"),
    }
}
pub(super) fn cpu_matches(e: &Expected, node: &Value) -> bool {
    let p = node.get("private_cpu");
    e.r == registers(node.get("registers"))
        && e.clock == node.get("clock_u64").number()
        && e.queue == queue(node.get("queue"))
        && e.state == state(p.get("physical_state"))
        && e.inspection
            == if p.get("physical_inspection").text() == "Unavailable" {
                PhysicalInspection::Unavailable
            } else {
                PhysicalInspection::DiagnosticCompleteValues
            }
        && e.frozen == p.get("frozen").flag()
        && e.stopped == p.get("stopped").flag()
        && e.pending_trace == p.get("pending_trace").flag()
        && e.level == p.get("interrupts").get("level").u16() as u8
        && e.edge == p.get("interrupts").get("level7_edge").flag()
        && e.pipeline_address
            == p.get("pipeline")
                .optional()
                .map(|pipeline| pipeline.get("address").u32())
        && e.queue
            == p.get("pipeline")
                .optional()
                .map(|pipeline| queue(pipeline.get("words")).unwrap())
        && e.imported == p.get("imported_boundary").optional().map(imported)
}
pub(super) fn ram_matches(words: &Ram, node: &Value) -> bool {
    words.iter().count() == node.list().len()
        && node.list().iter().all(|pair| {
            words.iter().any(|&(address, word)| {
                address == pair.list()[0].u32() && word == pair.list()[1].u16()
            })
        })
}
pub(super) fn events(e: &Expected, node: &Value) -> bool {
    e.events.len == node.list().len()
        && e.events
            .iter()
            .zip(node.list())
            .all(|(actual, literal)| *actual == event(literal))
}
pub(super) fn configure(b: &mut Device, node: &Value) {
    let mut primary = 0;
    let mut entry = 0;
    for raw in node.list() {
        if let Event::Transfer {
            stage,
            kind,
            elapsed,
            ..
        } = event(raw)
        {
            match stage {
                0 => {
                    b.primary[primary] = Report { kind, elapsed };
                    primary += 1;
                }
                2 => {
                    b.entry[entry] = Report { kind, elapsed };
                    entry += 1;
                }
                3 => b.secondary = Report { kind, elapsed },
                _ => panic!("literal script stage"),
            }
        }
    }
}
pub(super) fn case(row: &Value) -> (Case, Device) {
    let initial = row.get("initial");
    let r = registers(initial.get("registers"));
    let word = row.get("opcode").u16();
    let p = Profile::decode(word).unwrap();
    let words = ram(initial.get("word_RAM"));
    let c = Case {
        word,
        p,
        r,
        retained: initial.get("queue").list()[1].u16(),
        low: words.sample(r.pc.wrapping_add(4)),
        operand: 0,
        lane: 1,
        next: words.sample(r.pc.wrapping_add(p.length())),
        last: words.sample(r.pc.wrapping_add(p.length() + 2)),
        start: initial.get("clock_u64").number(),
    };
    let mut b = Device::seed(c);
    b.words = words;
    configure(&mut b, row.get("expected").get("events"));
    b.default_secondary = row.get("injection").get("default_entry_detector").flag();
    (c, b)
}
pub(super) fn primary(row: &Value) -> bool {
    let (c, b) = case(row);
    let e = super::oracle::predict(c, &b);
    let node = row.get("expected");
    let good = e.result == bridge::instruction(node.get("typed_result"))
        && cpu_matches(&e, node)
        && ram_matches(&e.words, node.get("word_RAM"))
        && events(&e, node.get("events"))
        && e.counts
            == [
                "instruction_transfers",
                "instruction_detectors",
                "entry_transfers",
                "entry_detectors",
                "idles",
            ]
            .map(|name| node.get("callback_counts").get(name).number() as usize)
        && e.source == Some(row.get("source_latched_numeric").u32())
        && e.compared == Some(row.get("actual_compared_numeric").u32())
        && capture_matches(&e, node)
        && before_matches(&e, node.get("before_imported_entry"))
        && script_matches(&e, node.get("device_image"));
    good && super::check::actual(c, b, &e, |_| {}).is_some()
}
pub(super) fn run() -> (u32, u32) {
    let packet = super::literal::packet();
    let mut tally = Tally::default();
    for row in packet.get("examples").list() {
        tally.add(primary(row));
    }
    for (route, row) in packet.get("preflight").list().iter().enumerate() {
        tally.add(preflight::run(route, row));
    }
    for (origin, row) in packet.get("reset_rows").list().iter().enumerate() {
        tally.add(resets::run(&packet, origin, row, 0));
    }
    for (route, row) in packet.get("reset_rejections").list().iter().enumerate() {
        tally.add(resets::run(&packet, route, row, 1));
    }
    for (detector, row) in packet.get("odd_reset_rows").list().iter().enumerate() {
        tally.add(resets::run(&packet, detector, row, 2));
    }
    assert_eq!(tally.0, 38);
    tally.finish()
}

pub(super) fn imported(n: &Value) -> zeff_m68k::timing::physical::ImportedGroup0Boundary {
    zeff_m68k::timing::physical::ImportedGroup0Boundary {
        registers: registers(n.get("registers")),
        clock: super::CpuClocks(n.get("clock_u64").number()),
        capture: bridge::capture(n.get("capture")),
    }
}

fn capture_matches(e: &Expected, node: &Value) -> bool {
    let capture = node.get("capture").optional();
    e.capture == capture.map(bridge::capture)
        && capture.is_none_or(|n| bridge::capture(n).special_status_word() == n.get("SSW").u16())
}
fn before_matches(e: &Expected, node: &Value) -> bool {
    match (&e.before_entry, node.optional()) {
        (None, None) => true,
        (Some((clock, words)), Some(n)) => {
            *clock == n.get("clock_u64").number() && ram_matches(words, n.get("word_RAM"))
        }
        _ => false,
    }
}
fn script_matches(e: &Expected, node: &Value) -> bool {
    let script = node.get("script").list();
    e.scripts.len == script.len()
        && e.scripts.iter().zip(script).all(|((_, kind, elapsed), n)| {
            *kind == self::kind(n.get("outcome")) && *elapsed == n.get("elapsed").u32()
        })
        && e.count == node.get("instruction_next").number() as usize
        && e.scripts.len == node.get("next").number() as usize
        && before_matches(e, node.get("before_entry"))
        && events(e, node.get("events"))
        && ram_matches(&e.words, node.get("word_RAM"))
        && e.clock == node.get("clock_u64").number()
}
