mod device;
mod intake;
mod plan;
mod provenance;

use crate::reference::Case;
use device::Device;
pub use intake::parse_jsonl;
use plan::Plan;
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::physical::{
    EntryKind, PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile,
};
use zeff_m68k::timing::physical_instruction::PhysicalInstructionStep;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    FirstOdd,
    Completion,
}

pub fn validate_original_payload(text: &str, selection: Selection) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let expected = match selection {
        Selection::FirstOdd => provenance::FIRST_ODD_SHA256,
        Selection::Completion => provenance::COMPLETION_SHA256,
    };
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != expected {
        return Err("compact payload SHA256 differs from original-fingerprint extraction".into());
    }
    Ok(())
}

pub fn verify(case: &Case, selection: Selection) -> Result<(), String> {
    let plan = Plan::new(case)?;
    if plan.odd != (selection == Selection::FirstOdd) {
        return Err("payload selection and actual EA parity differ".into());
    }
    let mut cpu = PrefetchCpu::from_prefetched(plan.initial, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial PC")?;
    let mut device = Device::new(&plan, &case.initial.ram)?;
    let result = cpu
        .step_physical_instruction(&mut device)
        .map_err(|error| format!("physical step: {error:?}"))?;
    let opcode = case.initial.prefetch[0];
    let clocks = match (selection, result) {
        (
            Selection::FirstOdd,
            PhysicalInstructionStep::Group0 {
                opcode: actual,
                capture,
                clocks,
            },
        ) => {
            let fc = if plan.initial.supervisor() { 5 } else { 1 };
            let ssw = (opcode & 0xffe0) | 16 | fc;
            if actual != opcode
                || capture.vector != 3
                || capture.internal_pc != plan.capture_pc
                || capture.phase_sr != plan.phase.status_register()
                || capture.ird != opcode
                || capture.logical_address != plan.operand
                || capture.function_code as u16 != fc
                || capture.direction != Direction::Read
                || capture.processing_class != ProcessingClass::InstructionOrGroup2
                || capture.ssw_profile != SswProfile::SelectedIrdUpperBits
                || capture.special_status_word() != ssw
            {
                return Err("full first-odd capture differs from manual phase".into());
            }
            clocks
        }
        (Selection::Completion, PhysicalInstructionStep::Instruction(step))
            if step.opcode == opcode && step.exception.is_none() =>
        {
            step.clocks
        }
        _ => return Err("physical completion/group0 outcome differs".into()),
    };
    let state = if plan.odd {
        PhysicalEntryState::Critical(EntryKind::ImportedGroup0)
    } else {
        PhysicalEntryState::Idle
    };
    let expected =
        PrefetchCpu::from_prefetched(plan.final_registers, plan.words, CpuClocks(plan.total))
            .ok_or("odd final PC")?;
    let mut private = format!("{expected:?}");
    let field = "physical_state: Idle,";
    if private.matches(field).count() != 1 {
        return Err("ambiguous expected physical state".into());
    }
    private = private.replacen(field, &format!("physical_state: {state:?},"), 1);
    if cpu.registers != plan.final_registers
        || cpu.queued_words() != Some(plan.words)
        || cpu.clock().0 != plan.total
        || clocks != plan.total
        || device.elapsed != plan.total
        || device.memory != plan.memory
        || device.cursor != plan.steps.len()
        || device.idle_remaining != 0
        || device.error.is_some()
        || cpu.trace_pending()
        || cpu.physical_entry_state() != state
        || cpu.physical_inspection() != PhysicalInspection::DiagnosticCompleteValues
        || format!("{cpu:?}") != private
    {
        return Err(
            "physical whole registers/queue/RAM/clocks/consumption/inspection differs".into(),
        );
    }
    Ok(())
}

pub fn verify_payload(text: &str, selection: Selection) -> Result<u32, String> {
    let cases = parse_jsonl(text)?;
    for case in &cases {
        verify(case, selection)
            .map_err(|error| format!("{}/{}: {error}", case.cohort, case.name))?;
    }
    u32::try_from(cases.len()).map_err(|_| "payload count overflow".into())
}

#[cfg(test)]
mod tests;
