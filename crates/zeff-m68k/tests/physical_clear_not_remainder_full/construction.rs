use super::device::Device;
use super::model::{Case, Failure, Report};
use super::{CpuClocks, PrefetchCpu, literal, mini, seed};

pub(super) fn reject(c: Case, bus: Device, mark: fn(bool)) -> bool {
    assert!(c.r.pc & 1 != 0);
    mark(true);
    let saved = bus.clone();
    let copied_bus = bus.clone();
    let input = (c.r, [c.word, c.retained], CpuClocks(c.start));
    let copied_input = input;
    let original = PrefetchCpu::from_prefetched(input.0, input.1, input.2);
    let copied = PrefetchCpu::from_prefetched(copied_input.0, copied_input.1, copied_input.2);
    let good = original.is_none()
        && copied.is_none()
        && input == copied_input
        && bus == saved
        && copied_bus == saved;
    mark(false);
    good
}

pub(super) fn mini() -> (u32, u32) {
    let rows = literal::construction_records();
    let mut bad = 0;
    for row in &rows {
        let a = row.get("axis");
        let tree = row.get("whole_tree");
        let input = tree.get("attempted_input");
        let device = tree.get("initial_device");
        let queue = mini::queue(input.get("queue")).unwrap();
        let mut c = seed::make(queue[0], true, 1);
        c.r = mini::registers(input.get("registers"));
        c.retained = queue[1];
        c.start = input.get("clock_u64").number();
        let mut bus = Device::seed(c);
        bus.fault(
            c.p.extensions() + 1,
            mini::kind(a.get("disposition")),
            a.get("durations").get("F").u32(),
        );
        let expected = tree.get("expected");
        let mut primary = [Report::default(); 6];
        for (to, from) in primary.iter_mut().zip(device.get("primary").list()) {
            *to = Report {
                kind: mini::kind(from.get("kind")),
                elapsed: from.get("elapsed").u32(),
            };
        }
        let mut untouched = Device::seed(c);
        untouched.primary = primary;
        let good = bus == untouched
            && mini::ram_matches(&bus.words, device.get("word_RAM"))
            && bus.clock == device.get("clock_u64").number()
            && bus.count == 0
            && bus.entry_count == 0
            && bus.events.len == 0
            && bus.scripts.len == 0
            && bus.secondary
                == Report {
                    kind: Failure::Boundary,
                    elapsed: 7,
                }
            && device == expected.get("original_device_after")
            && device == expected.get("copied_device_after")
            && input == expected.get("original_input_after")
            && input == expected.get("copied_input_after")
            && expected.get("cpu").optional().is_none()
            && expected.get("original_result").optional().is_none()
            && expected.get("owned_input_copy_result").optional().is_none()
            && expected.get("physical_steps").u32() == 0
            && reject(c, bus, |_| {});
        bad += u32::from(!good);
    }
    (rows.len() as u32, bad)
}
