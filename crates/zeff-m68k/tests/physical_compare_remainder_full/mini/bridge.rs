use super::super::literal::Value;
use super::super::{
    EntryFault, Group0Capture, InstructionFault, PhysicalInstructionStep, ProcessingClass,
    SswProfile, TimedStep, TimingFault,
};
use super::{access, cause, phase};
use zeff_m68k::timing::physical::{EntryInputError, EntryKind, EntryStep};

pub(super) fn capture(n: &Value) -> Group0Capture {
    assert_eq!(n.get("processing_class").text(), "InstructionOrGroup2");
    assert_eq!(n.get("direction").text(), "read");
    assert_eq!(n.get("ssw_profile").text(), "SelectedIrdUpperBits");
    Group0Capture {
        vector: n.get("vector").u16().try_into().unwrap(),
        internal_pc: n.get("PC").u32(),
        phase_sr: n.get("SR").u16(),
        ird: n.get("IRD").u16(),
        logical_address: n.get("EA").u32(),
        function_code: match n.get("FC").u16() {
            1 => super::super::FunctionCode::UserData,
            2 => super::super::FunctionCode::UserProgram,
            5 => super::super::FunctionCode::SupervisorData,
            6 => super::super::FunctionCode::SupervisorProgram,
            _ => panic!("literal capture FC"),
        },
        direction: super::super::Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}
fn entry(n: &Value) -> EntryFault<&'static str> {
    if let Value::Text(name) = n {
        return match *name {
            "EntryFault::ClockOverflow" => EntryFault::ClockOverflow,
            "EntryFault::HostFrozen" => EntryFault::HostFrozen,
            _ => panic!("literal entry unit"),
        };
    }
    let (name, f) = n.single();
    match name {
        "EntryFault::Input" => EntryFault::Input(match f.text() {
            "EntryInputError::UnqualifiedReset" => EntryInputError::UnqualifiedReset,
            "EntryInputError::NonmonotonicRelease" => EntryInputError::NonmonotonicRelease,
            _ => panic!("literal input"),
        }),
        "EntryFault::HostFailure" => {
            assert_eq!(f.get("source").text(), "host");
            EntryFault::HostFailure {
                access: access(f.get("access")),
                direction: if f.get("direction").text() == "read" {
                    super::super::Direction::Read
                } else {
                    super::super::Direction::Write
                },
                source: "host",
            }
        }
        "EntryFault::InvalidCompleteDuration" => EntryFault::InvalidCompleteDuration {
            elapsed: f.get("elapsed").u32(),
        },
        "EntryFault::DoubleFaultHalted" => EntryFault::DoubleFaultHalted(cause(f)),
        "EntryFault::TimingUnavailable" => EntryFault::TimingUnavailable(cause(f)),
        _ => panic!("literal entry fault"),
    }
}
pub(super) fn instruction(
    n: &Value,
) -> Result<PhysicalInstructionStep, InstructionFault<&'static str>> {
    let (ok, variant) = n.single();
    let (name, f) = variant.single();
    if ok == "Ok" {
        return Ok(match name {
            "PhysicalInstructionStep::Instruction" => {
                assert!(f.get("exception").optional().is_none());
                PhysicalInstructionStep::Instruction(TimedStep {
                    opcode: f.get("opcode").u16(),
                    clocks: f.get("clocks_u64").number(),
                    exception: None,
                })
            }
            "PhysicalInstructionStep::Group0" => PhysicalInstructionStep::Group0 {
                opcode: f.get("opcode").u16(),
                capture: capture(f.get("capture")),
                clocks: f.get("clocks_u64").number(),
            },
            _ => panic!("literal instruction step"),
        });
    }
    assert_eq!(ok, "Err");
    Err(match name {
        "InstructionFault::Entry" => InstructionFault::Entry(entry(f)),
        "InstructionFault::TransitionTimingUnavailable" => {
            InstructionFault::TransitionTimingUnavailable {
                phase: phase(f.get("phase")),
                cause: cause(f.get("cause")),
            }
        }
        "InstructionFault::CaptureUnavailable" => InstructionFault::CaptureUnavailable {
            phase: phase(f.get("phase")),
            cause: cause(f.get("cause")),
        },
        "InstructionFault::TraceUnqualified" => InstructionFault::TraceUnqualified,
        "InstructionFault::InterruptUnqualified" => InstructionFault::InterruptUnqualified,
        "InstructionFault::Execution" => {
            let (name, _) = f.single();
            InstructionFault::Execution(match name {
                "TimingFault::Unprimed" => TimingFault::Unprimed,
                "TimingFault::Stopped" => TimingFault::Stopped,
                "TimingFault::PendingTrace" => TimingFault::PendingTrace,
                "TimingFault::ImportedBoundaryPending" => TimingFault::ImportedBoundaryPending,
                _ => panic!("literal execution"),
            })
        }
        _ => panic!("literal instruction fault"),
    })
}
pub(super) fn reset(n: &Value) -> Result<EntryStep, EntryFault<&'static str>> {
    let (name, f) = n.single();
    if name == "Err" {
        return Err(entry(f));
    }
    assert_eq!(name, "Ok");
    let (name, f) = f.single();
    assert_eq!(name, "EntryStep");
    assert_eq!(f.get("kind").text(), "ExternalWarmReset");
    Ok(EntryStep {
        kind: EntryKind::ExternalWarmReset,
        clocks: f.get("clocks_u64").number(),
    })
}
