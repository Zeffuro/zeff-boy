use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[path = "verify_address_faults/intake.rs"]
mod intake;
#[path = "verify_address_faults/profile.rs"]
mod profile;
#[path = "verify_address_faults/witness.rs"]
mod witness;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: verify-address-faults [--tst|--cmp|--cmpi|--cmpa|--address-arithmetic|--data-arithmetic|--status-memory] <phase-jsonl> [receipt-json]")?;
    let mode = if first == "--status-memory" {
        intake::Mode::StatusMemory
    } else if first == "--data-arithmetic" {
        intake::Mode::DataArithmetic
    } else if first == "--address-arithmetic" {
        intake::Mode::AddressArithmetic
    } else if first == "--tst" {
        intake::Mode::Tst
    } else if first == "--cmpa" {
        intake::Mode::Cmpa
    } else if first == "--cmpi" {
        intake::Mode::Cmpi
    } else if first == "--cmp" {
        intake::Mode::Cmp
    } else {
        intake::Mode::Move
    };
    let path = if mode != intake::Mode::Move {
        args.next().ok_or("missing phase export")?
    } else {
        first
    };
    let receipt_path = args.next().map_or_else(
        || Path::new(&path).with_extension("receipt.json"),
        |path| Path::new(&path).to_path_buf(),
    );
    if args.next().is_some() {
        return Err("usage: verify-address-faults <phase-jsonl> [receipt-json]".into());
    }
    let report = match mode {
        intake::Mode::Move => intake::verify_reader(BufReader::new(File::open(&path)?))?,
        intake::Mode::Tst
        | intake::Mode::Cmp
        | intake::Mode::Cmpi
        | intake::Mode::Cmpa
        | intake::Mode::AddressArithmetic
        | intake::Mode::DataArithmetic
        | intake::Mode::StatusMemory => {
            intake::verify_reader_mode(BufReader::new(File::open(&path)?), mode)?
        }
    };
    let mut receipt = Vec::new();
    File::open(receipt_path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut receipt)?;
    if receipt.len() > 128 * 1024 {
        return Err("oversized address-fault receipt".into());
    }
    intake::verify_receipt(serde_json::from_slice(&receipt)?, &report)?;
    for (cohort, row) in &report.ledger {
        println!("{cohort}: {row:?}");
    }
    println!(
        "total: {} records, selected={}, selected_trace_excluded={}, passed={}, capture_gaps={}, failures={}; receipt_checked=true; sha256={}; raw_transaction_clocks={}",
        report.records,
        report.selected,
        report.selected_trace,
        report.passed,
        report.capture_gaps,
        report.failures,
        report.hash,
        report.raw_phase_clocks,
    );
    if !report.full_coverage() {
        return Err("selected full-cohort coverage differs from selected mode (MOVE 12500/2336/2325/2301/35; TST 7500/988/981/988/0; CMP 7500/712/703/712/0; CMPI 7500/66/84/66/0; CMPA 5000/822/844/822/0; ADDA/SUBA 10000/1621/1719/1621/0; data arithmetic 30000/1626/1633/1626/0; status 22500/725/715/725/0)".into());
    }
    println!(
        "Selected full address-fault suffixes and final state only. Detector supplies eight reference clocks through aser1; no physical-hardware detection timing claim."
    );
    if report.failures != 0 {
        return Err(format!("{} address-fault witness failures", report.failures).into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "verify_address_faults/tests.rs"]
mod tests;
