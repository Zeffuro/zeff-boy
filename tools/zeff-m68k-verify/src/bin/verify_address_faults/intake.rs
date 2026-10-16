use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{BufRead, Read};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeff_m68k_verify::reference::{Case, Phase};

pub(super) const COMMIT: &str = "64b253116a3de04aaac4346c43680960dc9b67e5";
pub(super) const FORMAT: &str = "m68000-reference-phases-jsonl";
pub(super) const MAX_LINE: usize = 64 * 1024;
const MAX_FILE: usize = 64 * 1024 * 1024;
#[path = "intake/status.rs"]
mod status;

const COHORTS: [&str; 5] = ["MOVE.b", "MOVE.w", "MOVE.l", "MOVEA.w", "MOVEA.l"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Move,
    Tst,
    Cmp,
    Cmpi,
    Cmpa,
    AddressArithmetic,
    DataArithmetic,
    StatusMemory,
}

impl Mode {
    fn cohorts(self) -> &'static [&'static str] {
        match self {
            Self::Move => &COHORTS,
            Self::StatusMemory => status::COHORTS,
            Self::Tst => &["TST.b", "TST.w", "TST.l"],
            Self::Cmp | Self::Cmpi => &["CMP.b", "CMP.w", "CMP.l"],
            Self::Cmpa => &["CMPA.w", "CMPA.l"],
            Self::AddressArithmetic => &["ADDA.w", "ADDA.l", "SUBA.w", "SUBA.l"],
            Self::DataArithmetic => &[
                "ADD.b", "ADD.w", "ADD.l", "SUB.b", "SUB.w", "SUB.l", "AND.b", "AND.w", "AND.l",
                "OR.b", "OR.w", "OR.l",
            ],
        }
    }
    fn maximum(self) -> u64 {
        self.cohorts().len() as u64 * 2500
    }
}

pub(super) fn source_hash(cohort: &str) -> Option<&'static str> {
    match cohort {
        "MOVE.b" => Some("b4400d361e6ef1f6dada70c613e95039161ad8142dfcb64b5cfa27bd5ee24959"),
        "MOVE.w" => Some("11c25e839789cb3902d12c0901b707fc5d039008424013375a03d6216efdf653"),
        "MOVE.l" => Some("5b3c5b8de96ca76bfc2b12033541f31048d12a17172f6f286f59314450e8cc15"),
        "MOVEA.w" => Some("2de2210c3d129584c292faf6d2124af1d59c2ab715ff9025f642bdf5e230c23e"),
        "MOVEA.l" => Some("3329a76915e9b78dbf8ef0db4260339ab0458c9b4bfb22aabddfcf0f534c344c"),
        "TST.b" => Some("289681ae49b0a58b412663488c288bade03748638399aeeb4ed8fdd64d17dc2d"),
        "TST.w" => Some("0d16c2cfd8112aa82fbdda4883635ba31dba5810e14288762e0744429c2c7a78"),
        "TST.l" => Some("a37cfd0f43451d39491bb8425c2fe46270e5d43cf46e5029a1fd4cd66df65541"),
        "CMPA.w" => Some("f49365f9816aa621f2a00b3e1b9eb96038221b1504a4c20bf5b5a4ec61cdee2f"),
        "CMPA.l" => Some("efd3c67626c60e4ec07e6174b8f98a8b302ba712310a041e3a516154524a40b2"),
        "ADDA.w" => Some("057ddac58789c8da190494a472cb6b591411d307b4d2363b31bb6ec195ee98e7"),
        "ADDA.l" => Some("a68dc7807e3705d56c9fd0615316c9fd17aed120fce74ef26fe4b796dfd0b3b2"),
        "SUBA.w" => Some("cbc43f9db2e44dd8d869345047b444773f73ed0af376723987b7faabca30d0cb"),
        "SUBA.l" => Some("788a2d5fac9563260f7771456425f484e812887a45a77b82d33cea1e70df4022"),
        "CMP.b" => Some("577ea026c89e6f8c722d343cc42eb524c43d6861d205ce45abe65d9bfbb86ce5"),
        "CMP.w" => Some("628adb87b4aa54c8f050ceb5be543139204461eb56502c15abaaa68560785d38"),
        "CMP.l" => Some("fa6c930fc6ac273308a29d1a3e535dc3be4632071299a572b7da934e5cb743d8"),
        "ADD.b" => Some("3d76bf7b85fc237be7ef2015445e91490677443d2bd4d6e6d84886989322cae5"),
        "ADD.w" => Some("bd596a3d2ed49b0e9619643dd1d85b07b05993465fde440ab011221561e629a2"),
        "ADD.l" => Some("cc19a1d614a9ce4a68e3e85a108a3d88811348a748b38950372c09bd947e711e"),
        "SUB.b" => Some("3201511e0398f360a6b27fc8ef3b04e747d5298aacdfdced8d6aba6dc39782f9"),
        "SUB.w" => Some("544902833ff09af419295b4a2bba70b5e55c2380b5444fe0107b89aa03fcb181"),
        "SUB.l" => Some("564976803df46dca0db22fcbdcc8ddfbd87d10ad6dcf62ed63f7c963b13da0e8"),
        "AND.b" => Some("dcc72a8ecf74ca6efc61a743ffdc475c448d8c0b517ae44b72044f6fc62246e4"),
        "AND.w" => Some("72f0e3ff75f1fff5f2ad221d5dcae703d7aff3256e77338a1a1594174a415e43"),
        "AND.l" => Some("bde6f203725d64dcd0e09244f5c127a0323ab16037626075cc965a3782fbcb38"),
        "OR.b" => Some("62add1222e705a287e23b4385ea45cc4ea9bd26f9b7c845d0cab06effaa9a1e3"),
        "OR.w" => Some("4d741c955f7edae2a2f5f12129f05fc28342a0ad5798552b2f177affa2cfd0d8"),
        "OR.l" => Some("649a3e0bc99758e01b237d28b7452f002fde28d770200034382f9d944a8182f4"),
        _ => status::source_hash(cohort),
    }
}

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

#[derive(Default, Debug)]
pub(super) struct Ledger {
    total: u64,
    eligible: u64,
    excluded: BTreeMap<String, u64>,
    inventory: BTreeMap<String, u64>,
    selected: u64,
    selected_trace: u64,
    passed: u64,
    capture_gaps: u64,
    failed: u64,
}

#[derive(Default, Debug)]
pub(super) struct Report {
    pub(super) mode: Mode,
    pub(super) ledger: BTreeMap<String, Ledger>,
    pub(super) records: u64,
    pub(super) selected: u64,
    pub(super) selected_trace: u64,
    pub(super) passed: u64,
    pub(super) capture_gaps: u64,
    pub(super) failures: u64,
    pub(super) hash: String,
    pub(super) raw_phase_clocks: u64,
}

impl Report {
    pub(super) fn full_coverage(&self) -> bool {
        let (selected, trace, frames, gaps) = match self.mode {
            Mode::StatusMemory => (725, 715, 725, 0),
            Mode::Move => (2336, 2325, 2301, 35),
            Mode::Tst => (988, 981, 988, 0),
            Mode::Cmp => (712, 703, 712, 0),
            Mode::Cmpi => (66, 84, 66, 0),
            Mode::Cmpa => (822, 844, 822, 0),
            Mode::AddressArithmetic => (1621, 1719, 1621, 0),
            Mode::DataArithmetic => (1626, 1633, 1626, 0),
        };
        self.records == self.mode.maximum()
            && self.selected == selected
            && self.selected_trace == trace
            && self.capture_gaps == gaps
            && self.passed + self.failures == frames
            && self.passed + self.failures + self.capture_gaps == self.selected
            && self.ledger.len() == self.mode.cohorts().len()
            && self.ledger.values().all(|row| row.total == 2500)
    }
}

fn cohort_matches(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    let address_destination = (opcode >> 6) & 7 == 1;
    match case.cohort.as_str() {
        "MOVE.b" => opcode >> 12 == 1 && !address_destination,
        "MOVE.w" => opcode >> 12 == 3 && !address_destination,
        "MOVE.l" => opcode >> 12 == 2 && !address_destination,
        "MOVEA.w" => opcode >> 12 == 3 && address_destination,
        "MOVEA.l" => opcode >> 12 == 2 && address_destination,
        "TST.b" => super::profile::legal_tst(opcode) && opcode >> 6 & 3 == 0,
        "TST.w" => super::profile::legal_tst(opcode) && opcode >> 6 & 3 == 1,
        "TST.l" => super::profile::legal_tst(opcode) && opcode >> 6 & 3 == 2,
        "CMPA.w" => super::profile::legal_cmpa(opcode) && opcode >> 6 & 7 == 3,
        "CMPA.l" => super::profile::legal_cmpa(opcode) && opcode >> 6 & 7 == 7,
        "ADDA.w" | "ADDA.l" | "SUBA.w" | "SUBA.l" => {
            let family = if case.cohort.starts_with("ADDA") {
                13
            } else {
                9
            };
            let opmode = if case.cohort.ends_with(".w") { 3 } else { 7 };
            super::profile::legal_address_arithmetic(opcode)
                && opcode >> 12 == family
                && opcode >> 6 & 7 == opmode
        }
        "CMP.b" | "CMP.w" | "CMP.l" => {
            let size = match case.cohort.as_str() {
                "CMP.b" => 0,
                "CMP.w" => 1,
                _ => 2,
            };
            let mode = opcode >> 3 & 7;
            super::profile::legal_cmp(opcode) && opcode >> 6 & 7 == size
                || opcode >> 12 == 11 && opcode >> 6 & 7 == size + 4 && mode == 1
                || super::profile::legal_cmpi(opcode) && opcode >> 6 & 3 == size
        }
        "ADD.b" | "ADD.w" | "ADD.l" | "SUB.b" | "SUB.w" | "SUB.l" | "AND.b" | "AND.w" | "AND.l"
        | "OR.b" | "OR.w" | "OR.l" => super::profile::data_cohort_matches(case),
        _ => status::cohort_matches(case),
    }
}

pub(super) fn selected(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    let source = (opcode >> 3) & 7;
    let destination = (opcode >> 6) & 7;
    matches!(opcode >> 12, 2 | 3)
        && ((matches!(destination, 0 | 1)
            && (matches!(source, 2..=6) || source == 7 && opcode & 7 <= 3))
            || ((source <= 1 || source == 7 && opcode & 7 == 4)
                && (matches!(destination, 2..=6) || destination == 7 && (opcode >> 9) & 7 <= 1))
            || ((matches!(destination, 2..=6) || destination == 7 && (opcode >> 9) & 7 <= 1)
                && (matches!(source, 2..=6) || source == 7 && opcode & 7 <= 3)))
}

pub(super) fn selected_tst(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    super::profile::legal_tst(opcode) && matches!(opcode >> 6 & 3, 1 | 2) && opcode >> 3 & 7 != 0
}

pub(super) fn selected_cmp(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    opcode >> 12 == 11
        && matches!(opcode >> 6 & 7, 1 | 2)
        && (matches!(opcode >> 3 & 7, 2..=6) || opcode >> 3 & 7 == 7 && opcode & 7 <= 3)
}

pub(super) fn selected_cmpa(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    super::profile::legal_cmpa(opcode)
        && (matches!(opcode >> 3 & 7, 2..=6) || opcode >> 3 & 7 == 7 && opcode & 7 <= 3)
}

pub(super) fn selected_cmpi(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    super::profile::legal_cmpi(opcode) && matches!(opcode >> 6 & 3, 1 | 2) && opcode >> 3 & 7 != 0
}

pub(super) fn selected_address_arithmetic(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    super::profile::legal_address_arithmetic(opcode)
        && (matches!(opcode >> 3 & 7, 2..=6) || opcode >> 3 & 7 == 7 && opcode & 7 <= 3)
}

pub(super) fn selected_data_arithmetic(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    super::profile::legal_data_arithmetic(opcode)
        && matches!(opcode >> 6 & 7, 1 | 2)
        && (matches!(opcode >> 3 & 7, 2..=6) || opcode >> 3 & 7 == 7 && opcode & 7 <= 3)
}

pub(super) fn selected_status_memory(case: &Case) -> bool {
    let opcode = case.initial.prefetch[0];
    let mode = opcode >> 3 & 7;
    super::profile::legal_status(opcode)
        && (matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 3)
        && (opcode & 0xffc0 == 0x44c0 || case.initial.sr & 0x2000 != 0)
}

fn record(report: &mut Report, case: &Case) -> Result<(), Box<dyn Error>> {
    if !cohort_matches(case) {
        return Err("opcode family or size contradicts source cohort".into());
    }
    case.initial.registers()?;
    case.expected.registers()?;
    for phase in &case.transactions {
        match phase {
            Phase::Idle { duration_cycles } if *duration_cycles == 0 => {
                return Err("zero raw idle duration".into());
            }
            Phase::Idle { .. } => {}
            Phase::Read(value)
            | Phase::Write(value)
            | Phase::Tas(value)
            | Phase::ReadAddressError(value)
            | Phase::WriteAddressError(value) => {
                if value.duration_cycles < 4
                    || value.address_bus > 0x00ff_ffff
                    || value.address_bus & 1 != 0
                    || value.data_bus > 0xffff
                    || value.uds > 1
                    || value.lds > 1
                    || value.uds + value.lds == 0
                    || !matches!(value.function_code, 1 | 2 | 5 | 6 | 7)
                {
                    return Err("invalid raw transfer fields".into());
                }
            }
        }
    }
    let clocks: u64 = case
        .transactions
        .iter()
        .map(|p| u64::from(p.duration()))
        .sum();
    if clocks != u64::from(case.reference_total_cycles) {
        return Err("raw transaction durations differ from reference total".into());
    }
    report.raw_phase_clocks = report
        .raw_phase_clocks
        .checked_add(clocks)
        .ok_or("raw phase clock sum overflow")?;
    let markers = case
        .transactions
        .iter()
        .filter(|phase| {
            matches!(
                phase,
                Phase::ReadAddressError(_) | Phase::WriteAddressError(_)
            )
        })
        .count();
    if markers > 1 || (case.initial.pc | case.expected.pc) & 1 != 0 && markers == 0 {
        return Err("unsupported or contradictory physical marker shape".into());
    }
    let physical = markers != 0;
    let traced = case.initial.sr & zeff_m68k::TRACE != 0;
    let low_supervisor = report.mode == Mode::StatusMemory && status::low_supervisor(case)
        || report.mode == Mode::Tst
            && case.cohort == "TST.b"
            && !traced
            && !physical
            && case.transactions.iter().any(|phase| {
                matches!(phase,
            Phase::Read(t) if t.function_code == 5 && t.address_bus < 0x400)
            });
    let exclusion = if traced {
        Some("trace")
    } else if physical {
        Some("address_fault")
    } else if low_supervisor {
        Some("exception_or_low_supervisor_data")
    } else {
        None
    };
    if case.normalization_exclusion.as_deref() != exclusion {
        return Err("normalization exclusion contradicts source evidence".into());
    }
    let row = report
        .ledger
        .get_mut(&case.cohort)
        .ok_or("case cohort not declared")?;
    row.total += 1;
    if let Some(reason) = exclusion {
        *row.excluded.entry(reason.into()).or_default() += 1;
    } else {
        row.eligible += 1;
    }
    let opcode = case.initial.prefetch[0];
    let admitted = match report.mode {
        Mode::StatusMemory => selected_status_memory(case),
        Mode::Move => selected(case),
        Mode::Tst => selected_tst(case),
        Mode::Cmp => selected_cmp(case),
        Mode::Cmpi => selected_cmpi(case),
        Mode::Cmpa => selected_cmpa(case),
        Mode::AddressArithmetic => selected_address_arithmetic(case),
        Mode::DataArithmetic => selected_data_arithmetic(case),
    } && physical;
    let inventory = format!(
        "{}:source_mode{}:destination_mode{}:trace{traced}:address_fault{physical}",
        if admitted { "selected" } else { "unselected" },
        (opcode >> 3) & 7,
        (opcode >> 6) & 7,
    );
    *row.inventory.entry(inventory).or_default() += 1;
    if !admitted {
        return Ok(());
    }
    super::witness::validate_profile(case)?;
    if traced {
        row.selected_trace += 1;
        report.selected_trace += 1;
        return Ok(());
    }
    row.selected += 1;
    report.selected += 1;
    match super::witness::run(case) {
        Ok(super::witness::WitnessResult::Frame) => {
            row.passed += 1;
            report.passed += 1;
        }
        Ok(super::witness::WitnessResult::CaptureGap) => {
            row.capture_gaps += 1;
            report.capture_gaps += 1;
        }
        Err(error) => {
            row.failed += 1;
            report.failures += 1;
            if report.failures <= 20 {
                eprintln!("{} / {}: {error}", case.cohort, case.name);
            }
        }
    }
    Ok(())
}

pub(super) fn verify_reader<R: BufRead>(reader: R) -> Result<Report, Box<dyn Error>> {
    verify_reader_mode(reader, Mode::Move)
}

pub(super) fn verify_reader_mode<R: BufRead>(
    mut reader: R,
    mode: Mode,
) -> Result<Report, Box<dyn Error>> {
    let mut report = Report {
        mode,
        ..Report::default()
    };
    let mut names = BTreeSet::new();
    let mut hash = Sha256::new();
    let (mut bytes, mut header, mut complete) = (0_usize, false, false);
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
            return Err("oversized address-fault phase export".into());
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
                    || value.claim.is_empty()
                    || value.pc_convention != "raw next-prefetch PC retained; no PC normalization"
                    || value.clock_convention
                        != "record total and ordered transaction durations retained separately; no synthesized timeline"
                    || value.ram_convention
                        != "16-bit source words retained as big-endian byte pairs"
                    || value.cohorts.len() != mode.cohorts().len()
                {
                    return Err("invalid address-fault phase header".into());
                }
                for cohort in value.cohorts {
                    if !mode.cohorts().contains(&cohort.as_str())
                        || source_hash(&cohort).is_none()
                        || report.ledger.insert(cohort, Ledger::default()).is_some()
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
                report.records += 1;
                if report.records > mode.maximum() {
                    return Err("too many address-fault records".into());
                }
                record(&mut report, &case)?;
            }
            Row::Complete(value) => {
                if !header || value.exported_records != report.records || report.records == 0 {
                    return Err("completion record count differs".into());
                }
                complete = true;
            }
        }
    }
    if !complete {
        return Err("address-fault export lacks completion footer".into());
    }
    report.hash = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    schema: u32,
    format: String,
    source: String,
    source_commit: String,
    license: String,
    license_sha256: String,
    readme_sha256: String,
    export_sha256: String,
    exported_records: u64,
    cohorts: Vec<SourceLedger>,
    claim: String,
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
