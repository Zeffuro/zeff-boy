#[allow(
    clippy::duplicate_mod,
    reason = "The bus uses each oracle's distinct Plan and Step types."
)]
#[path = "../status_register/device.rs"]
mod device;
mod plan;
mod provenance;

use crate::reference::Case;
use device::Device;
use plan::Plan;
use std::collections::BTreeSet;
use zeff_m68k::timing::physical_instruction::PhysicalInstructionStep;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

pub fn selected(case: &Case) -> bool {
    let word = case.initial.prefetch[0];
    let size = word >> 6 & 3;
    let mode = word >> 3 & 7;
    word & 0xff00 == 0x4a00
        && size < 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 1)
        && (size == 0 || mode == 0)
}

pub fn verify(case: &Case) -> Result<(), String> {
    let plan = Plan::new(case)?;
    let mut cpu = PrefetchCpu::from_prefetched(plan.initial, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial PC")?;
    let mut device = Device::new(&plan);
    let result = cpu
        .step_physical_instruction(&mut device)
        .map_err(|error| format!("physical TST: {error:?}"))?;
    if !matches!(result, PhysicalInstructionStep::Instruction(step)
        if step.opcode == case.initial.prefetch[0] && step.exception.is_none() && step.clocks == plan.clocks)
    {
        return Err("physical TST typed completion differs".into());
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
        return Err("physical TST whole CPU/RAM/events/clocks/consumption differs".into());
    }
    Ok(())
}

pub fn verify_export(text: &str) -> Result<u32, String> {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != provenance::EXPORT_SHA256 {
        return Err("original TST export hash differs".into());
    }
    let mut keys = BTreeSet::new();
    let mut cohorts = [0u32; 3];
    let (mut excluded, mut count) = (0, 0);
    for line in text.lines() {
        let mut raw: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if raw.get("type").and_then(|v| v.as_str()) != Some("case") {
            continue;
        }
        raw.as_object_mut()
            .ok_or("invalid original record")?
            .remove("type");
        let case: Case = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        if !keys.insert((case.cohort.clone(), case.name.clone())) {
            return Err("duplicate original identity".into());
        }
        if !selected(&case) {
            continue;
        }
        if case.initial.sr & 0x8000 != 0 {
            excluded += 1;
            continue;
        }
        let size = usize::from(case.initial.prefetch[0] >> 6 & 3);
        cohorts[size] += 1;
        verify(&case).map_err(|e| format!("{}/{}: {e}", case.cohort, case.name))?;
        count += 1;
    }
    if keys.len() != 7500 || cohorts != [1266, 221, 211] || excluded != 1635 || count != 1698 {
        return Err("original TST selection partition differs".into());
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
