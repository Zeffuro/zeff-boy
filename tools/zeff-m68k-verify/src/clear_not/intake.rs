use std::collections::BTreeSet;

use crate::reference::{Case, Phase};

pub(super) fn validate(case: &Case) -> Result<(), String> {
    let (family, size) = match case.cohort.as_str() {
        "CLR.w" => (0x4200, 1),
        "CLR.l" => (0x4200, 2),
        "NOT.w" => (0x4600, 1),
        "NOT.l" => (0x4600, 2),
        _ => return Err("unselected CLR/NOT cohort".into()),
    };
    let op = case.initial.prefetch[0];
    let mode = op >> 3 & 7;
    if op & 0xff00 != family
        || op >> 6 & 3 != size
        || !(matches!(mode, 2..=6) || mode == 7 && op & 7 <= 1)
    {
        return Err("cohort/opcode/selected memory route mismatch".into());
    }
    if case.initial.sr & 0x8000 != 0 {
        return Err("trace raw execution is excluded".into());
    }
    if case.name.is_empty() || case.initial.pc & 1 != 0 || case.expected.pc & 1 != 0 {
        return Err("invalid raw identity or next-prefetch PC".into());
    }
    case.initial.registers()?;
    case.expected.registers()?;
    let mut markers = 0;
    let mut clocks = 0u64;
    for phase in &case.transactions {
        clocks = clocks
            .checked_add(u64::from(phase.duration()))
            .ok_or("phase clock sum overflow")?;
        match phase {
            Phase::Idle { duration_cycles } => {
                if *duration_cycles == 0 {
                    return Err("zero raw idle".into());
                }
            }
            Phase::Read(t) | Phase::Write(t) | Phase::ReadAddressError(t) => {
                if matches!(phase, Phase::ReadAddressError(_)) {
                    markers += 1;
                }
                if t.duration_cycles != 4
                    || t.address_bus > 0x00ff_ffff
                    || t.address_bus & 1 != 0
                    || !matches!(t.function_code, 1 | 2 | 5 | 6)
                    || t.data_bus > 65535
                    || (t.uds, t.lds) != (1, 1)
                {
                    return Err("invalid raw Word transfer fields".into());
                }
            }
            _ => return Err("unselected TAS/write-address-error phase".into()),
        }
    }
    if clocks != u64::from(case.reference_total_cycles)
        || markers > 1
        || case.normalization_exclusion.as_deref()
            != if markers == 1 {
                Some("address_fault")
            } else {
                None
            }
    {
        return Err("raw normalization/marker count/clock total differs".into());
    }
    Ok(())
}

pub fn parse_jsonl(text: &str) -> Result<Vec<Case>, String> {
    let mut keys = BTreeSet::new();
    let mut cases = Vec::new();
    for (line, value) in text.lines().enumerate() {
        if value.is_empty() {
            return Err(format!("empty payload line {}", line + 1));
        }
        let case: Case =
            serde_json::from_str(value).map_err(|error| format!("line {}: {error}", line + 1))?;
        validate(&case)?;
        if !keys.insert((case.cohort.clone(), case.name.clone())) {
            return Err("duplicate (cohort,name) before execution".into());
        }
        cases.push(case);
    }
    if cases.is_empty() {
        return Err("empty CLR/NOT payload".into());
    }
    Ok(cases)
}
