use super::*;
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};

#[path = "sound_matrix/cases.rs"]
mod cases;
#[path = "sound_matrix/oracle.rs"]
mod oracle;
#[path = "sound_matrix/runner.rs"]
mod runner;
use cases::{Case, Kind};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_source_first() {
    runner::run(Case::new("E-qualified", Kind::Immediate, 0, 28, 0, false));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_qualified_1439_other() {
    let mut count = 0;
    for kind in [Kind::Immediate, Kind::PairLow, Kind::PairHigh] {
        for residue in 0..15 {
            for edge in [0, 27, 28, 29, 41, 42, 69, 70] {
                for lane in 0..2 {
                    for disjoint in [false, true] {
                        if kind == Kind::Immediate
                            && residue == 0
                            && edge == 28
                            && lane == 0
                            && !disjoint
                        {
                            continue;
                        }
                        runner::run(Case::new(
                            "E-qualified",
                            kind,
                            residue,
                            edge,
                            lane,
                            disjoint,
                        ));
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 1439);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_unclassified_360() {
    let mut count = 0;
    for kind in [Kind::Ldi, Kind::BankM1, Kind::BankPsg] {
        for residue in 0..15 {
            for edge in [0, 27, 28, 29, 41, 42, 69, 70] {
                runner::run(Case::new("F", kind, residue, edge, 0, false));
                count += 1;
            }
        }
    }
    assert_eq!(count, 360);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_endpoints_270() {
    let mut count = 0;
    for residue in 0..15 {
        for endpoint in [28, 42, 70] {
            for offset in [-1, 0, 1] {
                for end_target in [false, true] {
                    let kind = if end_target {
                        Kind::Ldi
                    } else {
                        Kind::Immediate
                    };
                    let edge = endpoint + offset - if end_target { 45 } else { 0 };
                    runner::run(Case::new("E-F-edges", kind, residue, edge, 0, false));
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 270);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_local_60() {
    let mut count = 0;
    for kind in [
        Kind::LocalRead,
        Kind::LocalWrite,
        Kind::LocalPsg,
        Kind::LocalOpn2,
    ] {
        for residue in 0..15 {
            runner::run(Case::new("F-local", kind, residue, -30, 0, false));
            count += 1;
        }
    }
    assert_eq!(count, 60);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_devices_2() {
    for kind in [Kind::BankOpn2, Kind::BankRegister] {
        runner::run(Case::new("F-device", kind, 0, 28, 0, false));
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_sound_matrix_epoch_trace_4() {
    for blocked in [false, true] {
        for trace in [false, true] {
            let mut case = Case::new("F-epoch-trace", Kind::LocalPsg, 0, -30, 0, false);
            case.epoch = Some(blocked);
            case.trace = trace;
            runner::run(case);
        }
    }
}
