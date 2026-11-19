mod device;
mod plan;
mod provenance;

use crate::reference::Case;
use device::Device;
use plan::Plan;
use std::collections::BTreeSet;
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

pub fn validate_original_payload(text: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != provenance::COMPLETION_SHA256 {
        return Err("original compact status payload hash differs".into());
    }
    Ok(())
}

pub fn parse_jsonl(text: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    let mut keys = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let case: Case =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        if !keys.insert((case.cohort.clone(), case.name.clone())) {
            return Err("duplicate cohort/name".into());
        }
        Plan::new(&case)?;
        cases.push(case);
    }
    if cases.is_empty() {
        return Err("empty status payload".into());
    }
    Ok(cases)
}

pub fn verify(case: &Case) -> Result<(), String> {
    let plan = Plan::new(case)?;
    let mut cpu = PrefetchCpu::from_prefetched(plan.initial, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial PC")?;
    let mut device = Device::new(&plan);
    let result = cpu
        .step_physical_instruction(&mut device)
        .map_err(|error| format!("physical status: {error:?}"))?;
    if !matches!(result, PhysicalInstructionStep::Instruction(step)
        if step.opcode == case.initial.prefetch[0] && step.exception.is_none() && step.clocks == plan.clocks)
    {
        return Err("physical typed completion differs".into());
    }
    let expected = PrefetchCpu::from_prefetched(plan.expected, plan.queue, CpuClocks(plan.clocks))
        .ok_or("odd final PC")?;
    if cpu != expected
        || device.clocks != plan.clocks
        || device.cursor != plan.steps.len()
        || device.actual != plan.steps
        || device.memory != plan.memory
        || device.error.is_some()
    {
        return Err("physical whole CPU/RAM/events/clocks/consumption differs".into());
    }
    if plan.expected.status_register() & 0x8000 != 0 {
        let before = cpu.clone();
        let original_device = device.clone();
        let mut copied_cpu = before.clone();
        let mut copied_device = original_device.clone();
        if cpu.step_physical_instruction(&mut device) != Err(InstructionFault::TraceUnqualified)
            || copied_cpu.step_physical_instruction(&mut copied_device)
                != Err(InstructionFault::TraceUnqualified)
            || cpu != before
            || copied_cpu != before
            || device != original_device
            || copied_device != original_device
        {
            return Err("new trace boundary or copied retry changed whole state".into());
        }
    }
    Ok(())
}

pub fn verify_payload(text: &str) -> Result<u32, String> {
    let cases = parse_jsonl(text)?;
    for case in &cases {
        verify(case).map_err(|error| format!("{}/{}: {error}", case.cohort, case.name))?;
    }
    u32::try_from(cases.len()).map_err(|_| "status payload count overflow".into())
}

#[cfg(test)]
mod tests;
