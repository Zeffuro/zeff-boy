use super::super::fixed::Event;
use super::{Case, Device, Profile, Value, cpu_matches, ram, ram_matches, registers};

pub(super) fn run(route: usize, row: &Value) -> bool {
    let before = row.get("before");
    let mut r = registers(before.get("registers"));
    if route == 0 || route == 6 {
        r.set_status_register(r.status_register() & !0x8000);
    }
    if route == 3 {
        r.pc = r.pc.wrapping_sub(2);
    }
    let node = before.get("private_cpu").get("pipeline");
    let words = if node.optional().is_some() {
        super::queue(node.get("words")).unwrap()
    } else {
        let m = ram(before.get("word_RAM"));
        [m.sample(r.pc), m.sample(r.pc.wrapping_add(2))]
    };
    let p = Profile::decode(words[0]).unwrap();
    let m = ram(before.get("word_RAM"));
    let c = Case {
        word: words[0],
        retained: words[1],
        p,
        r,
        low: m.sample(r.pc.wrapping_add(4)),
        next: m.sample(r.pc.wrapping_add(p.length())),
        last: m.sample(r.pc.wrapping_add(p.length() + 2)),
        operand: 0,
        lane: 1,
        start: 37,
    };
    let mut config = Device::seed(c);
    config.words = m;
    let Some((cpu, b, e, fault)) = super::super::preflight::prepare(c, config, route, |_| {})
    else {
        return false;
    };
    let d = before.get("device_image");
    let good = cpu_matches(&e, before)
        && ram_matches(&b.words, d.get("word_RAM"))
        && b.clock == d.get("clock_u64").number()
        && b.ordinary
            == [
                d.get("ordinary_callback_counts").get("reads").number() as usize,
                d.get("ordinary_callback_counts").get("idles").number() as usize,
            ]
        && b.counts == [0; 5]
        && b.events.len == d.get("events").list().len()
        && b.events
            .iter()
            .zip(d.get("events").list())
            .all(|(actual, n)| {
                *actual
                    == if n.get("kind").text() == "ordinary_idle" {
                        Event::Idle(n.get("start_clock").number(), n.get("elapsed").u32())
                    } else {
                        Event::Transfer {
                            stage: 4,
                            phase: None,
                            access: super::access(n.get("access")),
                            direction: super::super::Direction::Read,
                            value: Some(n.get("value").u16()),
                            start: n.get("start_clock").number(),
                            elapsed: n.get("elapsed").u32(),
                            kind: super::Failure::Complete,
                            deposited: false,
                        }
                    }
            })
        && Err::<super::super::PhysicalInstructionStep, _>(fault.clone())
            == super::bridge::instruction(row.get("expected_result"))
        && cpu_matches(&e, row.get("expected_CPU_device"));
    good && super::super::preflight::attempt(cpu, b, fault, |_| {})
}
