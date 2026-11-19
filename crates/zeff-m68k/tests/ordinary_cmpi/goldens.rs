use super::*;
use fixture::{Case, Outcome, access, registers};

const PATHS: [&str; 7] = [
    "complete_wait0",
    "complete_wait3",
    "FHost_wait0",
    "FHost_wait3",
    "Fblocked_after_MAX_operand",
    "F_exact_MAX_success",
    "source_capacity",
];
fn record(count: &mut u32, bad: &mut u32, case: Case, supervisor: bool, path: &str) {
    *count += 1;
    let good = check::run(case);
    *bad += u32::from(!good);
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary CMPI literal {} bank={} path={path}",
            *count, supervisor
        );
    }
    let _ = (supervisor, path);
}
fn word(supervisor: bool, path: usize) -> Case {
    let starts = [
        0,
        0,
        0,
        0,
        18446744065119617012,
        18446744069414584306,
        18446744069414584315,
    ];
    let ends = [
        14,
        23,
        14,
        23,
        18446744069414584317,
        u64::MAX,
        18446744069414584321,
    ];
    let start = starts[path];
    let wait = if matches!(path, 1 | 3) { 3 } else { 0 };
    let r = registers(supervisor, 0x3002);
    let mut c = Case::new("Wpre", 0x0c67, r, start, false);
    c.read(start, 0x1004, (false, 0x4e71, wait));
    c.idle(start + 4 + u64::from(wait));
    if path != 6 {
        c.read(
            start + 6 + u64::from(wait),
            0x3000,
            (true, 0x7fff, if path == 4 { u32::MAX } else { wait }),
        );
    }
    if path < 4 || path == 5 {
        c.read(
            start + 10 + 2 * u64::from(wait),
            0x1006,
            (false, 0x7201, if path == 5 { u32::MAX } else { wait }),
        );
    }
    let sr = if supervisor {
        if matches!(path, 4 | 6) {
            0x2714
        } else {
            0x271b
        }
    } else if matches!(path, 4 | 6) {
        0x0714
    } else {
        0x071b
    };
    let outcome = match path {
        2 | 3 => Outcome::Host(access(0x1006, supervisor, false)),
        4 | 6 => Outcome::Overflow,
        _ => Outcome::Complete,
    };
    c.finish(0x3000, sr, ends[path], outcome);
    c
}
fn long(supervisor: bool, path: usize) -> Case {
    let starts = [
        0,
        0,
        0,
        0,
        18446744065119617006,
        18446744069414584300,
        18446744069414584308,
    ];
    let ends = [
        20,
        35,
        20,
        35,
        18446744069414584317,
        u64::MAX,
        18446744069414584320,
    ];
    let start = starts[path];
    let wait = if matches!(path, 1 | 3) { 3 } else { 0 };
    let r = registers(supervisor, 0x3000);
    let mut c = Case::new("Lpost", 0x0c9f, r, start, true);
    c.read(start, 0x1004, (false, 1, wait));
    c.read(start + 4 + u64::from(wait), 0x1006, (false, 0x4e71, wait));
    c.read(
        start + 8 + 2 * u64::from(wait),
        0x3000,
        (true, 0x8000, wait),
    );
    if path != 6 {
        c.read(
            start + 12 + 3 * u64::from(wait),
            0x3002,
            (true, 0, if path == 4 { u32::MAX } else { wait }),
        );
    }
    if path < 4 || path == 5 {
        c.read(
            start + 16 + 4 * u64::from(wait),
            0x1008,
            (false, 0x7201, if path == 5 { u32::MAX } else { wait }),
        );
    }
    let sr = if supervisor {
        if matches!(path, 4 | 6) {
            0x2714
        } else {
            0x2712
        }
    } else if matches!(path, 4 | 6) {
        0x0714
    } else {
        0x0712
    };
    let outcome = match path {
        2 | 3 => Outcome::Host(access(0x1008, supervisor, false)),
        4 | 6 => Outcome::Overflow,
        _ => Outcome::Complete,
    };
    c.finish(0x3004, sr, ends[path], outcome);
    c
}
fn word_post(supervisor: bool) -> Case {
    let start = 18446744069414584316;
    let mut c = Case::new(
        "Wpost-source-capacity",
        0x0c5f,
        registers(supervisor, 0x3000),
        start,
        false,
    );
    c.read(start, 0x1004, (false, 0x4e71, 0));
    c.finish(
        0x3002,
        if supervisor { 0x2714 } else { 0x0714 },
        18446744069414584320,
        Outcome::Overflow,
    );
    c
}
fn long_pre(supervisor: bool) -> Case {
    let start = 18446744069414584312;
    let mut c = Case::new(
        "Lpre-source-capacity",
        0x0ca7,
        registers(supervisor, 0x3004),
        start,
        true,
    );
    c.read(start, 0x1004, (false, 1, 0));
    c.read(start + 4, 0x1006, (false, 0x4e71, 0));
    c.idle(start + 8);
    c.finish(
        0x3000,
        if supervisor { 0x2714 } else { 0x0714 },
        18446744069414584322,
        Outcome::Overflow,
    );
    c
}
pub(super) fn run() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for supervisor in [false, true] {
        for (path, name) in PATHS.into_iter().enumerate() {
            record(
                &mut count,
                &mut bad,
                word(supervisor, path),
                supervisor,
                name,
            );
        }
        record(
            &mut count,
            &mut bad,
            word_post(supervisor),
            supervisor,
            "complementarysource",
        );
    }
    for supervisor in [false, true] {
        for (path, name) in PATHS.into_iter().enumerate() {
            record(
                &mut count,
                &mut bad,
                long(supervisor, path),
                supervisor,
                name,
            );
        }
        record(
            &mut count,
            &mut bad,
            long_pre(supervisor),
            supervisor,
            "complementarysource",
        );
    }
    (count, bad + u32::from(count != 32))
}
