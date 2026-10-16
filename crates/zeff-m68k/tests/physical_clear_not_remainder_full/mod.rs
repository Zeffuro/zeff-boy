use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{
    CriticalFault, EntryBus, EntryFault, EntryKind, EntryOutcome, EntryReport, Group0Capture,
    PhysicalEntryState, PhysicalInspection, ProcessingClass, SswProfile, TimingQualification,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionPhase, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu, TimedStep, TimingFault};

mod capacity;
mod check;
#[path = "../physical_tst_remainder_full/classifier.rs"]
#[allow(clippy::duplicate_mod)]
mod classifier;
mod collision_products;
mod construction;

pub fn construction_mini_section() -> (u32, u32) {
    construction::mini()
}

pub const ALLOCATION_COUNTS: [u32; 11] = [
    884000, 109272, 50240, 77760, 1728, 41448, 38880, 7536, 12960, 180864, 17280,
];

pub fn allocation_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = if index == 0 {
        capacity::repeated(mark)
    } else {
        section_with(
            [28, 29, 30, 32, 33, 34, 35, 36, 37, 39][index as usize - 1],
            mark,
        )
    };
    assert_eq!(result.0, ALLOCATION_COUNTS[index as usize]);
    result
}
mod device;
mod entry;
mod entry_products;
mod fixed;
mod followup;
mod literal;
mod mini;
mod model;
mod oracle;
mod predicates;
mod reset_products;
mod status;

pub const COUNTS: [u32; 40] = [
    65536, 424, 4752, 1769472, 964608, 60288, 80384, 94208, 16384, 32768, 640, 2097152, 163840,
    2097152, 262144, 1536, 10368, 10752, 20096, 10368, 18840, 3768, 896, 224, 24128, 6032, 432,
    258048, 109272, 50240, 77760, 25920, 1728, 41448, 38880, 7536, 12960, 180864, 13824, 17280,
];

pub fn section_with(index: u32, mark: fn(bool)) -> (u32, u32) {
    let result = match index {
        0..=2 | 38 => guard_section(index, mark),
        3..=9 | 11 | 13 | 14 => primary_section(index, mark),
        10 | 12 | 15..=27 => directed_section(index, mark),
        28 | 29 | 33 | 35 => entry_products::run(index, mark),
        30..=32 | 34 | 36 | 39 => reset_products::run(index, mark),
        37 => collision_products::run(index, mark),
        _ => panic!("full section"),
    };
    assert_eq!(result.0, COUNTS[index as usize]);
    result
}

pub fn status_mini_section() -> (u32, u32) {
    status::mini()
}
mod preflight;
mod seed;

pub fn guard_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let (expected, result) = match index {
        0 => (65536, predicates::run(mark)),
        1 => (424, page_section(mark)),
        2 => (4752, preflight::run(mark)),
        38 => (13824, followup::run(mark)),
        _ => panic!("guard section"),
    };
    assert_eq!(result.0, expected);
    result
}

fn page_section(mark: fn(bool)) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for page in [0x4200, 0x4600] {
        for field in 0..256 {
            let word = page | field;
            let mode = word >> 3 & 7;
            let reg = word & 7;
            if word >> 6 & 3 < 3 && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 1) {
                continue;
            }
            for bank in [false, true] {
                let good = if classifier::selected(word, bank) {
                    status::run(word, mark)
                } else {
                    followup::unsupported(word, bank, mark)
                };
                count += 1;
                bad += u32::from(!good);
            }
        }
    }
    (count, bad)
}

pub fn mini_section() -> (u32, u32) {
    mini::run()
}

pub fn directed_mini_section() -> (u32, u32) {
    let mut bad = 0;
    let rows = literal::directed_records();
    for row in &rows {
        let good = mini::primary(row);
        bad += u32::from(!good);
        if !good {
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("directed literal {} failed", row.get("id").text());
        }
    }
    (rows.len() as u32, bad)
}

pub fn factor_source_section() -> (u32, u32) {
    let rows = literal::factor_records();
    let mut bad = 0;
    for row in &rows {
        bad += u32::from(!mini::source_matches(row));
    }
    (rows.len() as u32, bad)
}

mod directed;
mod matrix;

pub const DIRECTED_COUNTS: [(u32, u32); 15] = [
    (10, 640),
    (12, 163840),
    (15, 1536),
    (16, 10368),
    (17, 10752),
    (18, 20096),
    (19, 10368),
    (20, 18840),
    (21, 3768),
    (22, 896),
    (23, 224),
    (24, 24128),
    (25, 6032),
    (26, 432),
    (27, 258048),
];

pub fn directed_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let count = DIRECTED_COUNTS
        .iter()
        .find(|&&(section, _)| section == index)
        .expect("directed section")
        .1;
    let result = if matches!(index, 12 | 15) {
        directed::aliases(index, mark)
    } else {
        directed::run(index, mark)
    };
    assert_eq!(result.0, count);
    result
}

pub const PRIMARY_COUNTS: [(u32, u32); 10] = [
    (3, 1769472),
    (4, 964608),
    (5, 60288),
    (6, 80384),
    (7, 94208),
    (8, 16384),
    (9, 32768),
    (11, 2097152),
    (13, 2097152),
    (14, 262144),
];

pub fn primary_section(index: u32, mark: fn(bool)) -> (u32, u32) {
    let count = PRIMARY_COUNTS
        .iter()
        .find(|&&(section, _)| section == index)
        .expect("primary section")
        .1;
    let result = if matches!(index, 11 | 13 | 14) {
        matrix::extensions(index, mark)
    } else {
        matrix::run(index, mark)
    };
    assert_eq!(result.0, count);
    result
}
