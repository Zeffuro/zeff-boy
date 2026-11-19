use super::super::{check, device::DefaultSecondary, entry, model::Report, oracle};
use super::{Value, cpu_matches, events, ram_matches};
use zeff_m68k::timing::physical::{ExternalResetBoundary, ResetQualification};

fn device_matches(
    e: &super::Expected,
    n: &Value,
    reset_entries: usize,
    reset_detectors: usize,
    device_clock: u64,
) -> bool {
    let counts = n.get("callback_counts");
    ram_matches(&e.words, n.get("word_RAM"))
        && events(e, n.get("events"))
        && device_clock == n.get("clock_u64").number()
        && e.counts[0] == counts.get("instruction_transfers").number() as usize
        && e.counts[1] == counts.get("instruction_detectors").number() as usize
        && e.counts[2] == counts.get("entry_transfers").number() as usize + reset_entries
        && e.counts[3] == counts.get("entry_detectors").number() as usize + reset_detectors
        && e.counts[4] == counts.get("idles").number() as usize
        && e.entries == reset_entries
        && e.scripts.len == n.get("next").number() as usize
        && e.scripts
            .iter()
            .zip(n.get("script").list())
            .all(|((_, kind, elapsed), row)| {
                *kind == super::kind(row.get("outcome")) && *elapsed == row.get("elapsed").u32()
            })
}

pub(super) fn run(raw: &Value, row: &Value) -> bool {
    let (c, config) = super::case(raw);
    let mut e = oracle::predict(c, &config);
    if !super::expected_matches(&e, raw) {
        return false;
    }
    let Some((mut cpu, mut b)) = check::actual(c, config, &e, |_| {}) else {
        return false;
    };
    let private = row.get("reset_input_CPU").get("private_cpu");
    let interrupts = private.get("interrupts");
    e.level = interrupts.get("level").u16() as u8;
    e.edge = interrupts.get("level7_edge").flag();
    if e.edge {
        cpu.interrupt_input().set_level(7);
    }
    cpu.interrupt_input().set_level(e.level);
    if !cpu_matches(&e, row.get("reset_input_CPU")) {
        return false;
    }
    b.entry_count = 0;
    e.entries = 0;
    b.entry = [Report::default(); 16];
    b.entry_deposit = [false; 16];
    b.default_secondary = true;
    super::configure(&mut b, row.get("reset_events"));
    let request = row.get("reset_request");
    let release = row.get("reset_input_device").get("clock_u64").number();
    let qualified = request.find("qualified").is_none_or(Value::flag);
    b.clock = release;
    let boundary = ExternalResetBoundary {
        sampled_release: super::super::CpuClocks(release),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    };
    let expected = entry::warm_reset(&mut e, &b, release, qualified);
    let device_clock = if row.get("rejected").flag() {
        release
    } else {
        e.clock
    };
    if expected != super::bridge::reset(row.get("typed_result"))
        || !cpu_matches(&e, row.get("expected_CPU"))
        || !device_matches(
            &e,
            row.get("expected_device"),
            row.get("reset_entries").number() as usize,
            row.get("reset_detectors").number() as usize,
            device_clock,
        )
    {
        return false;
    }
    let mut copied = cpu.clone();
    let mut copied_bus = b.clone();
    let actual_copy = if b.default_secondary {
        copied.external_warm_reset(&mut DefaultSecondary(&mut copied_bus), boundary)
    } else {
        copied.external_warm_reset(&mut copied_bus, boundary)
    };
    let actual = if b.default_secondary {
        cpu.external_warm_reset(&mut DefaultSecondary(&mut b), boundary)
    } else {
        cpu.external_warm_reset(&mut b, boundary)
    };
    actual == expected
        && actual_copy == expected
        && cpu == copied
        && b == copied_bus
        && check::verify_at(&cpu, &b, &e, device_clock)
        && e.guard
            .clone()
            .is_none_or(|fault| check::guards(&mut cpu, &mut b, fault, |_| {}))
}
