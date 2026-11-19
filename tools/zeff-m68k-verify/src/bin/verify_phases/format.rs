use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{BufRead, Read};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[path = "admission.rs"]
mod admission;
pub(super) use admission::source_hash;
use admission::{cohort_matches, route_unsupported};
pub(super) fn transfer_metadata(case: &Case) -> bool {
    cohort_matches(case)
}

pub(super) const COMMIT: &str = "64b253116a3de04aaac4346c43680960dc9b67e5";
pub(super) const FORMAT: &str = "m68000-reference-phases-jsonl";
pub(super) const MAX_LINE: usize = 64 * 1024;
const PC_CONVENTION: &str = "raw next-prefetch PC retained; no PC normalization";
const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_RECORDS: u64 = 50_000;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Row {
    Header(Header),
    Case(Box<Case>),
    Complete(Complete),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    format: String,
    source_commit: String,
    cohorts: Vec<String>,
    claim: String,
    pc_convention: String,
    clock_convention: String,
    ram_convention: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    exported_records: u64,
}

pub(super) use zeff_m68k_verify::reference::{Case, Phase, Transfer};

#[derive(Default, Debug)]
pub(super) struct Ledger {
    pub(super) total: u64,
    pub(super) eligible: u64,
    pub(super) excluded: BTreeMap<String, u64>,
    pub(super) unsupported: u64,
    pub(super) passed: u64,
    pub(super) trace_prefix_pass: u64,
    pub(super) physical_unsupported: u64,
    pub(super) unsupported_routes: BTreeMap<String, u64>,
}

pub(super) struct Report {
    pub(super) ledger: BTreeMap<String, Ledger>,
    pub(super) records: u64,
    pub(super) failures: u64,
    pub(super) hash: String,
    pub(super) raw_phase_clocks: u64,
}

pub(super) fn verify_reader<R: BufRead>(mut reader: R) -> Result<Report, Box<dyn Error>> {
    let mut ledger = BTreeMap::<String, Ledger>::new();
    let mut names = BTreeSet::new();
    let mut hash = Sha256::new();
    let (mut records, mut failures, mut bytes, mut raw_phase_clocks) =
        (0_u64, 0_u64, 0_usize, 0_u64);
    let (mut header, mut complete) = (false, false);
    loop {
        let mut line = Vec::new();
        (&mut reader)
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            break;
        }
        bytes += line.len();
        if line.len() > MAX_LINE || bytes > MAX_FILE {
            return Err("oversized phase export".into());
        }
        if complete {
            return Err("data after completion footer".into());
        }
        hash.update(&line);
        match serde_json::from_slice::<Row>(&line)? {
            Row::Header(value) => {
                if header
                    || value.schema != 1
                    || value.format != FORMAT
                    || value.source_commit != COMMIT
                    || value.cohorts.is_empty()
                    || value.claim.is_empty()
                    || value.pc_convention != PC_CONVENTION
                    || value.clock_convention.is_empty()
                    || value.ram_convention.is_empty()
                {
                    return Err("invalid phase header".into());
                }
                for cohort in value.cohorts {
                    if source_hash(&cohort).is_none()
                        || ledger.insert(cohort, Ledger::default()).is_some()
                    {
                        return Err("unknown or duplicate source cohort".into());
                    }
                }
                header = true;
            }
            Row::Case(case) => {
                if !header || !names.insert((case.cohort.clone(), case.name.clone())) {
                    return Err("case before header or duplicate case name".into());
                }
                records += 1;
                if records > MAX_RECORDS {
                    return Err("too many phase records".into());
                }
                raw_phase_clocks = raw_phase_clocks
                    .checked_add(
                        case.transactions
                            .iter()
                            .map(|phase| u64::from(phase.duration()))
                            .sum(),
                    )
                    .ok_or("raw phase clock sum overflow")?;
                let row = ledger
                    .get_mut(&case.cohort)
                    .ok_or("case cohort not declared")?;
                row.total += 1;
                if !cohort_matches(&case) {
                    return Err("opcode family or size contradicts source cohort".into());
                }
                let traced = case.initial.sr & zeff_m68k::TRACE != 0;
                let physical = case.initial.pc & 1 != 0
                    || case.expected.pc & 1 != 0
                    || case.transactions.iter().any(|phase| {
                        matches!(
                            phase,
                            Phase::ReadAddressError(_) | Phase::WriteAddressError(_)
                        )
                    });
                let baseline = matches!(case.cohort.as_str(), "NOP" | "MOVE.q" | "Bcc");
                if baseline {
                    match case.normalization_exclusion.as_deref() {
                        Some("trace") if traced => {
                            *row.excluded.entry("trace".into()).or_default() += 1;
                            if physical {
                                row.physical_unsupported += 1;
                                continue;
                            }
                        }
                        Some("address_fault") if !traced && physical => {
                            *row.excluded.entry("address_fault".into()).or_default() += 1;
                            continue;
                        }
                        None if !traced && !physical => row.eligible += 1,
                        _ => {
                            return Err(
                                "normalization exclusion contradicts baseline evidence".into()
                            );
                        }
                    }
                } else {
                    if traced != (case.normalization_exclusion.as_deref() == Some("trace")) {
                        return Err("trace exclusion contradicts initial SR".into());
                    }
                    if let Some(reason) = &case.normalization_exclusion {
                        if !matches!(
                            reason.as_str(),
                            "trace"
                                | "address_fault"
                                | "exception_or_low_supervisor_data"
                                | "prefetch_ram_disagreement"
                                | "data_prefetch_overlap"
                                | "self_modifying_code"
                        ) {
                            return Err("unknown source exclusion".into());
                        }
                        *row.excluded.entry(reason.clone()).or_default() += 1;
                    } else {
                        row.eligible += 1;
                    }
                    if let Some(reason) = route_unsupported(&case) {
                        *row.unsupported_routes.entry(reason.into()).or_default() += 1;
                        continue;
                    }
                    if physical {
                        row.physical_unsupported += 1;
                        continue;
                    }
                }
                let result = super::run(&case);
                match result {
                    Ok(()) => {
                        if traced {
                            row.trace_prefix_pass += 1;
                        } else {
                            row.passed += 1;
                        }
                    }
                    Err(error) => {
                        failures += 1;
                        if failures <= 20 {
                            eprintln!("{} / {}: {error}", case.cohort, case.name);
                        }
                    }
                }
            }
            Row::Complete(value) => {
                if !header || value.exported_records != records || records == 0 {
                    return Err("completion record count differs".into());
                }
                complete = true;
            }
        }
    }
    if !complete {
        return Err("phase export lacks completion footer".into());
    }
    Ok(Report {
        ledger,
        records,
        failures,
        hash: hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        raw_phase_clocks,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) schema: u32,
    pub(super) format: String,
    pub(super) source: String,
    pub(super) source_commit: String,
    pub(super) license: String,
    pub(super) license_sha256: String,
    pub(super) readme_sha256: String,
    pub(super) export_sha256: String,
    pub(super) exported_records: u64,
    cohorts: Vec<SourceLedger>,
    pub(super) claim: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceLedger {
    cohort: String,
    raw_sha256: String,
    total: u64,
    eligible: u64,
    excluded: BTreeMap<String, u64>,
    exported: u64,
}

pub(super) fn verify_receipt(receipt: Receipt, report: &Report) -> Result<(), Box<dyn Error>> {
    if receipt.schema != 1
        || receipt.format != FORMAT
        || receipt.source_commit != COMMIT
        || receipt.source
            != format!("https://raw.githubusercontent.com/SingleStepTests/m68000/{COMMIT}/")
        || receipt.license != "MIT"
        || receipt.claim.is_empty()
        || receipt.license_sha256
            != "9b9e332ee11019f131e945d40e20ab0c48b52921754dc03fe62cc85b579e6c61"
        || receipt.readme_sha256
            != "7f36eed9cb93f061d13bfd18dc310c9bda7b5f3ac96b6d0dfe292e00612c46b6"
        || receipt.export_sha256 != report.hash
        || receipt.exported_records != report.records
        || receipt.cohorts.len() != report.ledger.len()
    {
        return Err("source receipt or export hash differs".into());
    }
    let mut seen = BTreeSet::new();
    for cohort in receipt.cohorts {
        let actual = report
            .ledger
            .get(&cohort.cohort)
            .ok_or("receipt cohort differs")?;
        if !seen.insert(cohort.cohort.clone())
            || source_hash(&cohort.cohort) != Some(cohort.raw_sha256.as_str())
            || cohort.total != actual.total
            || cohort.exported != actual.total
            || cohort.eligible != actual.eligible
            || cohort.excluded != actual.excluded
        {
            return Err("source cohort pin or coverage ledger differs".into());
        }
    }
    Ok(())
}
