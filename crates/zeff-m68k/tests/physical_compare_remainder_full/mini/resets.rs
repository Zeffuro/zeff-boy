use super::super::{check, oracle, reset};
use super::{Value, cpu_matches, events, ram_matches};

fn device(e: &super::Expected, n: &Value) -> bool {
    ram_matches(&e.words, n.get("word_RAM"))
        && events(e, n.get("events"))
        && e.counts
            == [
                "instruction_transfers",
                "instruction_detectors",
                "entry_transfers",
                "entry_detectors",
                "idles",
            ]
            .map(|name| n.get("callback_counts").get(name).number() as usize)
}
pub(super) fn run(packet: &Value, index: usize, row: &Value, group: usize) -> bool {
    let origin = if group == 0 {
        index
    } else if group == 1 && index == 2 {
        1
    } else {
        0
    };
    let raw = &packet.get("reset_origins").list()[origin];
    let (c, mut config) = super::case(raw);
    if group == 1 && index == 3 {
        reset::configure(&mut config, c, 5);
    }
    if group == 2 {
        for pair in packet.get("odd_reset_initial_setup").list() {
            config
                .words
                .insert(pair.list()[0].u32(), pair.list()[1].u16());
        }
    }
    let mut e = oracle::predict(c, &config);
    if group == 0 {
        let node = raw.get("expected");
        if e.result != super::bridge::instruction(node.get("typed_result"))
            || !cpu_matches(&e, node)
            || !ram_matches(&e.words, node.get("word_RAM"))
            || !events(&e, node.get("events"))
        {
            return false;
        }
    }
    let Some((mut cpu, mut b)) = check::actual(c, config, &e, |_| {}) else {
        return false;
    };
    reset::preparation(&mut b);
    super::configure(&mut b, row.get("reset_events"));
    if group == 2 {
        reset::detector(&mut b, index);
    }
    for level in row.get("IRQ_setup").list() {
        cpu.interrupt_input()
            .set_level(level.u16().try_into().unwrap());
    }
    e.level = row
        .get("initial_CPU")
        .get("private_cpu")
        .get("interrupts")
        .get("level")
        .u16()
        .try_into()
        .unwrap();
    e.edge = row
        .get("initial_CPU")
        .get("private_cpu")
        .get("interrupts")
        .get("level7_edge")
        .flag();
    e.entries = 0;
    let boundary = row.get("reset_boundary");
    let release = boundary.get("sampled_release").number();
    b.clock = release;
    if !cpu_matches(&e, row.get("initial_CPU"))
        || !device(&e, row.get("initial_device"))
        || b.clock != row.get("initial_device").get("clock_u64").number()
        || !check::verify_at(&cpu, &b, &e, release)
    {
        return false;
    }
    reset::execute(
        cpu,
        b,
        e,
        reset::boundary(
            release,
            boundary.get("qualification").text() != "Unqualified",
        ),
        |_| {},
        |expected, result| {
            *result == super::bridge::reset(row.get("typed_result"))
                && cpu_matches(expected, row.get("expected_CPU"))
                && device(expected, row.get("expected_device"))
        },
    )
}
