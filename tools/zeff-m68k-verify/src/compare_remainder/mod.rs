#[allow(
    clippy::duplicate_mod,
    reason = "The bus uses each oracle's distinct Plan and Step types."
)]
#[path = "../status_register/device.rs"]
mod device;
mod plan;
mod source;

use crate::reference::Case;
use device::Device;
use plan::Plan;
use std::collections::BTreeSet;
use zeff_m68k::timing::physical_instruction::PhysicalInstructionStep;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

const EXPORT_SHA256: &str = "89b83bb7956f72ceec290167c67f5b2384e896fff75847a0b37e61b1fb065689";
const SOURCE_COMMIT: &str = "64b253116a3de04aaac4346c43680960dc9b67e5";

pub fn selected(case: &Case) -> bool {
    let word = case.initial.prefetch[0];
    let opmode = word >> 6 & 7;
    let mode = word >> 3 & 7;
    word & 0xf000 == 0xb000
        && matches!(opmode, 0..=3 | 7)
        && !(opmode == 0 && mode == 1)
        && !(mode == 7 && word & 7 > 4)
        && (opmode == 0 || mode < 2 || mode == 7 && word & 7 == 4)
}

fn verify_plan(case: &Case, plan: &Plan) -> Result<(), String> {
    let mut cpu = PrefetchCpu::from_prefetched(plan.initial, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial PC")?;
    let mut device = Device::new(plan);
    let result = cpu
        .step_physical_instruction(&mut device)
        .map_err(|error| format!("physical comparison: {error:?}"))?;
    if !matches!(result, PhysicalInstructionStep::Instruction(step)
        if step.opcode == case.initial.prefetch[0] && step.exception.is_none() && step.clocks == plan.clocks)
    {
        return Err("physical comparison typed completion differs".into());
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
        return Err("physical comparison whole CPU/RAM/events/clocks/consumption differs".into());
    }
    Ok(())
}

pub fn verify(case: &Case) -> Result<(), String> {
    verify_plan(case, &Plan::new(case)?)
}

fn export(text: &str, execute: bool) -> Result<u32, String> {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != EXPORT_SHA256 {
        return Err("original comparison export hash differs".into());
    }
    let mut keys = BTreeSet::new();
    let mut words = BTreeSet::new();
    let mut selected_words = BTreeSet::new();
    let mut original = [0u32; 5];
    let mut completed = [0u32; 5];
    let mut excluded = [0u32; 5];
    let mut header = false;
    for line in text.lines() {
        let mut raw: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if raw.get("type").and_then(|v| v.as_str()) == Some("header") {
            if header || raw.get("source_commit").and_then(|v| v.as_str()) != Some(SOURCE_COMMIT) {
                return Err("original comparison header differs".into());
            }
            header = true;
            continue;
        }
        let cohort = match raw.get("cohort").and_then(|v| v.as_str()) {
            Some("CMP.b") => 0,
            Some("CMP.w") => 1,
            Some("CMP.l") => 2,
            Some("CMPA.w") => 3,
            Some("CMPA.l") => 4,
            _ => continue,
        };
        if raw.get("type").and_then(|v| v.as_str()) != Some("case") {
            return Err("original comparison case kind differs".into());
        }
        raw.as_object_mut()
            .ok_or("invalid original record")?
            .remove("type");
        let case: Case = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        if !keys.insert((case.cohort.clone(), case.name.clone())) {
            return Err("duplicate original comparison identity".into());
        }
        original[cohort] += 1;
        if !selected(&case) {
            continue;
        }
        selected_words.insert(case.initial.prefetch[0]);
        if case.initial.sr & 0x8000 != 0 {
            if case.normalization_exclusion.as_deref() != Some("trace") {
                return Err("original old-T tag differs".into());
            }
            excluded[cohort] += 1;
            continue;
        }
        let plan = Plan::new(&case).map_err(|e| format!("{}/{}: {e}", case.cohort, case.name))?;
        if execute {
            verify_plan(&case, &plan).map_err(|e| format!("{}/{}: {e}", case.cohort, case.name))?;
        }
        completed[cohort] += 1;
        words.insert(case.initial.prefetch[0]);
    }
    if !header
        || original != [2500; 5]
        || completed != [1016, 275, 270, 344, 351]
        || excluded != [975, 281, 301, 358, 339]
        || selected_words.len() != 959
        || words.len() != 879
    {
        return Err("original comparison remainder partition differs".into());
    }
    Ok(completed.into_iter().sum())
}

pub fn qualify_export(text: &str) -> Result<u32, String> {
    export(text, false)
}

pub fn verify_export(text: &str) -> Result<u32, String> {
    export(text, true)
}

#[cfg(test)]
mod tests;
