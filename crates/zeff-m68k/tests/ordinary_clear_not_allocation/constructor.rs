use crate::allocation_support::{Windows, positive_control};
use crate::axis;
use crate::oracle::{self, Expected};
use crate::seed::{self, Axis};
use std::hint::black_box;
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

fn expected() -> Vec<Expected> {
    let mut rows = Vec::new();
    let mut odd = 0;
    axis::each(25, |value| {
        let row = oracle::predict(seed::build(&Axis::from_literal(&value)));
        assert!(!row.importable);
        rows.push(row);
        odd += 1;
    });
    assert_eq!(odd, 1296);
    let mut emitted = 0;
    let mut rejected = 0;
    axis::each(16, |value| {
        let row = oracle::predict(seed::build(&Axis::from_literal(&value)));
        emitted += 1;
        if !row.importable {
            rows.push(row);
            rejected += 1;
        }
    });
    assert_eq!((emitted, rejected), (7776, 30));
    assert_eq!(rows.len(), 1326);
    rows
}

fn execute(row: &Expected, windows: &mut Windows) {
    assert!(!row.importable);
    let initial = &row.initial;
    let input = (initial.r, initial.queue, CpuClocks(initial.device.clock));
    let saved_input = input;
    let device = initial.device.clone();
    let saved_device = device.clone();
    assert_eq!(input.0.pc & 1, 1);
    assert_eq!(row.device, device);
    assert_eq!(
        (device.events.len, device.scripts.len, device.transfers),
        (0, 0, 0)
    );
    let (copied_input, copied_device) =
        windows.measure(|| (black_box(input), black_box(device.clone())));
    assert_eq!(input, saved_input);
    assert_eq!(copied_input, saved_input);
    assert_eq!(device, saved_device);
    assert_eq!(copied_device, saved_device);
    let original = windows.measure(|| {
        PrefetchCpu::from_prefetched(black_box(input.0), black_box(input.1), black_box(input.2))
    });
    assert!(original.is_none());
    assert_eq!(input, saved_input);
    assert_eq!(copied_input, saved_input);
    assert_eq!(device, saved_device);
    assert_eq!(copied_device, saved_device);
    let copied = windows.measure(|| {
        PrefetchCpu::from_prefetched(
            black_box(copied_input.0),
            black_box(copied_input.1),
            black_box(copied_input.2),
        )
    });
    assert!(copied.is_none());
    assert_eq!(input, saved_input);
    assert_eq!(copied_input, saved_input);
    assert_eq!(device, saved_device);
    assert_eq!(copied_device, saved_device);
}

#[test]
fn all_constructor1326_requests3978_windows() {
    positive_control();
    let rows = expected();
    let mut windows = Windows::default();
    let mut requests = 0;
    for row in &rows {
        execute(row, &mut windows);
        requests += 1;
    }
    assert_eq!((requests, windows.count), (1326, 3978));
    println!(
        "constructor requests={requests} windows={} delta=0 positive_allocator_control=PASS",
        windows.count
    );
}
