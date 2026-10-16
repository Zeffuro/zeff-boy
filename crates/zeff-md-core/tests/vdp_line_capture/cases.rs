#![allow(dead_code)]

#[path = "../vdp_line/support.rs"]
mod support;

use support::*;
use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, Vdp, VdpInterruptError, VdpLineError, VdpRenderError, VdpScanline, VdpScheduleError,
    VdpTimingError, VdpVerticalTiming,
};

type Capture = Result<VdpScanline, VdpLineError>;

fn render(error: VdpRenderError) -> VdpLineError {
    VdpLineError::Render(error)
}

fn schedule(error: VdpTimingError) -> VdpLineError {
    VdpLineError::Schedule(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
        error,
    )))
}

fn reject(
    capture: &mut impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
    device: &mut Vdp,
    at: MasterTicks,
    region: Region,
    error: VdpLineError,
) {
    let saved = device.clone();
    let pointer = device.memory().vram().as_ptr();
    assert_eq!(capture(device, at, region), Err(error));
    assert_eq!(*device, saved);
    assert_eq!(device.memory().vram().as_ptr(), pointer);
}

fn slots(device: &Vdp, count: usize) -> MasterTicks {
    let timing = VdpVerticalTiming::at_counters_zero(tick(0));
    let mut at = device.now();
    for _ in 0..count {
        at = timing
            .next_memory_slot(at, REGION, device.registers())
            .unwrap();
    }
    at
}

fn late_fixture() -> Vdp {
    let mut device = fixture();
    register(&mut device, 17, 1);
    words(&mut device, 0xf000, 1, &[1]);
    assert!(
        device
            .memory()
            .sample_background(device.registers(), 15, ROW)
            .is_ok()
    );
    assert_eq!(
        device
            .memory()
            .sample_background(device.registers(), 16, ROW),
        Err(VdpRenderError::UnsupportedWindowScroll)
    );
    device
}

fn enqueue(device: &mut Vdp, address: u32, code: u8, word: u16) {
    command(device, address, code);
    device.write_data(word, REGION).unwrap();
}

pub fn publication_deadlines_and_replay() {
    publication_deadlines_and_replay_with(Vdp::capture_line_at);
}

pub fn publication_deadlines_and_replay_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut device = fixture();
    let old = capture(&mut device, tick(ROW_AT), REGION).unwrap();
    assert_eq!(old.colors(), &std::array::from_fn::<_, 256, _>(expected));
    let immutable = old.clone();
    let write = device.queue_register(7, 14, REGION).unwrap();
    let mut whole = device.clone();
    let mut ordinary = device.clone();
    let pointer = device.memory().vram().as_ptr();
    for at in [
        write.publish_at.get() - 1,
        write.publish_at.get(),
        write.publish_at.get() + 1,
    ] {
        let row = capture(&mut device, tick(at), REGION).unwrap();
        assert_eq!((row.at(), row.row()), (tick(at), ROW));
        for x in 0..256 {
            assert_eq!(
                row.colors()[x],
                if x >= 32 && at >= write.publish_at.get() {
                    color(14)
                } else {
                    expected(x)
                }
            );
        }
        assert_eq!(device.memory().vram().as_ptr(), pointer);
    }
    let at = device.now();
    let row = capture(&mut whole, at, REGION).unwrap();
    ordinary.advance_to(at, REGION).unwrap();
    assert_eq!(row, ordinary.prepare_line(REGION).unwrap());
    assert_eq!(whole, device);
    assert_eq!(ordinary, device);
    assert_eq!(capture(&mut device, at, Region::NtscJapan).unwrap(), row);
    let stale = tick(at.get() - 1);
    reject(
        &mut capture,
        &mut device,
        stale,
        REGION,
        VdpLineError::Schedule(VdpScheduleError::Interrupt(
            VdpInterruptError::TimeReversal {
                current: at,
                requested: stale,
            },
        )),
    );
    assert_eq!(old, immutable);
    assert_eq!(old.colors(), &std::array::from_fn::<_, 256, _>(expected));
    device.advance_to(tick(at.get() + 1), REGION).unwrap();
    enqueue(&mut device, 30, 3, 0xeeee);
    let current = device.now();
    reject(
        &mut capture,
        &mut device,
        at,
        REGION,
        VdpLineError::Schedule(VdpScheduleError::Interrupt(
            VdpInterruptError::TimeReversal {
                current,
                requested: at,
            },
        )),
    );
}

pub fn fifo_completion_is_inclusive() {
    fifo_completion_is_inclusive_with(Vdp::capture_line_at);
}

pub fn fifo_completion_is_inclusive_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut device = fixture();
    let old = device.prepare_line(REGION).unwrap();
    enqueue(&mut device, 30, 3, 0xeeee);
    let end = slots(&device, 1);
    reject(
        &mut capture,
        &mut device,
        tick(end.get() - 1),
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    let mut ordinary = device.clone();
    ordinary.advance_to(end, REGION).unwrap();
    let pointer = device.memory().vram().as_ptr();
    let row = capture(&mut device, end, REGION).unwrap();
    assert_eq!(device, ordinary);
    assert_eq!(row, ordinary.prepare_line(REGION).unwrap());
    assert_eq!(row.at(), end);
    assert_eq!(device.memory().cram()[15], 0x0eee);
    assert!(device.fifo().is_empty());
    assert_eq!(device.memory().vram().as_ptr(), pointer);
    for x in 0..256 {
        assert_eq!(row.colors()[x], if x >= 32 { 0x0eee } else { expected(x) });
    }
    assert_eq!(old.colors(), &std::array::from_fn::<_, 256, _>(expected));
}

pub fn aliased_vram_rollback() {
    aliased_vram_rollback_with(Vdp::capture_line_at);
}

pub fn aliased_vram_rollback_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut device = late_fixture();
    register(&mut device, 15, 0);
    for (address, code, value) in [
        (0x80, 1, 0x1234),
        (0x81, 1, 0x5678),
        (0x80, 0x11, 0x9abc),
        (0x81, 0x11, 0xdef0),
    ] {
        enqueue(&mut device, address, code, value);
    }
    assert!(device.fifo().is_full());
    let end = slots(&device, 8);
    for at in [slots(&device, 1), tick(end.get() - 1)] {
        reject(
            &mut capture,
            &mut device,
            at,
            REGION,
            render(VdpRenderError::PendingLineInputs),
        );
    }
    for _ in 0..2 {
        reject(
            &mut capture,
            &mut device,
            end,
            REGION,
            render(VdpRenderError::UnsupportedWindowScroll),
        );
    }
    let mut ordinary = device.clone();
    ordinary.advance_to(end, REGION).unwrap();
    assert_eq!(&ordinary.memory().vram()[0x80..0x82], &[0xf0, 0xde]);
    let fix = device.queue_register(17, 0, REGION).unwrap();
    let at = end.max(fix.publish_at);
    let row = capture(&mut device, at, REGION).unwrap();
    assert!(device.fifo().is_empty());
    assert_eq!(&device.memory().vram()[0x80..0x82], &[0xf0, 0xde]);
    assert_eq!(row.colors()[255], color(15));
}

pub fn mixed_memory_and_sprite_rollback() {
    mixed_memory_and_sprite_rollback_with(Vdp::capture_line_at);
}

pub fn mixed_memory_and_sprite_rollback_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut mixed = late_fixture();
    enqueue(&mut mixed, 30, 3, 0xeeee);
    enqueue(&mut mixed, 0, 5, 0x0555);
    enqueue(&mut mixed, 0x80, 1, 0x1234);
    enqueue(&mut mixed, 0x81, 0x11, 0x5678);
    let end = slots(&mixed, 6);
    reject(
        &mut capture,
        &mut mixed,
        end,
        REGION,
        render(VdpRenderError::UnsupportedWindowScroll),
    );

    let mut aliases = late_fixture();
    for (address, code, value) in [
        (30, 3, 0x2222),
        (31, 0x13, 0xeeee),
        (0, 5, 0x0111),
        (1, 0x15, 0x0666),
    ] {
        enqueue(&mut aliases, address, code, value);
    }
    let end = slots(&aliases, 4);
    reject(
        &mut capture,
        &mut aliases,
        end,
        REGION,
        render(VdpRenderError::UnsupportedWindowScroll),
    );
    let mut ordinary = aliases.clone();
    ordinary.advance_to(end, REGION).unwrap();
    assert_eq!(ordinary.memory().cram()[15], 0x0eee);
    assert_eq!(ordinary.memory().vsram()[0], 0x0666);

    let mut sprite = fixture();
    let cached = *sprite.sprite_cache().unwrap();
    enqueue(&mut sprite, 0x200a, 1, 1);
    let end = slots(&sprite, 2);
    reject(
        &mut capture,
        &mut sprite,
        end,
        REGION,
        render(VdpRenderError::SpriteLinkCycle(1)),
    );
    sprite.advance_to(end, REGION).unwrap();
    assert_ne!(*sprite.sprite_cache().unwrap(), cached);
    assert_eq!(
        sprite.prepare_line(REGION),
        Err(render(VdpRenderError::SpriteLinkCycle(1)))
    );
}

pub fn read_completion_is_inclusive() {
    read_completion_is_inclusive_with(Vdp::capture_line_at);
}

pub fn read_completion_is_inclusive_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut device = fixture();
    command(&mut device, 32, 0);
    let first = slots(&device, 1);
    let end = device.data_read_wait(REGION).unwrap().unwrap();
    reject(
        &mut capture,
        &mut device,
        first,
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    device.advance_to(first, REGION).unwrap();
    assert!(!device.read_cache().is_ready());
    reject(
        &mut capture,
        &mut device,
        tick(end.get() - 1),
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    let mut ordinary = device.clone();
    ordinary.advance_to(end, REGION).unwrap();
    let row = capture(&mut device, end, REGION).unwrap();
    assert_eq!(row.colors(), &std::array::from_fn::<_, 256, _>(expected));
    assert_eq!(device, ordinary);
    assert!(device.read_cache().is_ready());
    assert_eq!(capture(&mut device, end, REGION).unwrap(), row);
    device.advance_to(tick(end.get() + 1), REGION).unwrap();
    assert_eq!(device.read_data(REGION).unwrap(), 0x1234);

    for queued_write in [false, true] {
        let mut late = late_fixture();
        if queued_write {
            enqueue(&mut late, 32, 1, 0x9876);
        }
        command(&mut late, 32, 0);
        let end = late.data_read_wait(REGION).unwrap().unwrap();
        reject(
            &mut capture,
            &mut late,
            end,
            REGION,
            render(VdpRenderError::UnsupportedWindowScroll),
        );
        let mut ordinary = late.clone();
        ordinary.advance_to(end, REGION).unwrap();
        assert!(ordinary.read_cache().is_ready());
        assert_eq!(
            ordinary.read_cache().buffered_word(),
            Some(if queued_write { 0x9876 } else { 0x1234 })
        );
    }
}

pub fn schedule_and_render_guards() {
    schedule_and_render_guards_with(Vdp::capture_line_at);
}

pub fn schedule_and_render_guards_with(
    mut capture: impl FnMut(&mut Vdp, MasterTicks, Region) -> Capture,
) {
    let mut bootstrap = Vdp::bootstrap_mode5();
    reject(
        &mut capture,
        &mut bootstrap,
        tick(0),
        REGION,
        schedule(VdpTimingError::UnsupportedMode { index: 1, value: 4 }),
    );
    for index in [1, 12] {
        let mut pending = fixture();
        let write = pending
            .queue_register(index, if index == 1 { 0x44 } else { 0x10 }, REGION)
            .unwrap();
        if index == 1 {
            assert_eq!(
                capture(&mut pending, tick(write.publish_at.get() - 1), REGION)
                    .unwrap()
                    .colors(),
                &std::array::from_fn::<_, 256, _>(expected)
            );
        } else {
            reject(
                &mut capture,
                &mut pending,
                tick(write.publish_at.get() - 1),
                REGION,
                schedule(VdpTimingError::PendingModeChange),
            );
        }
        assert_eq!(
            capture(&mut pending, write.publish_at, REGION)
                .unwrap()
                .colors(),
            &std::array::from_fn::<_, 256, _>(expected)
        );
    }
    let mut history = fixture();
    let write = history.queue_register(1, 4, REGION).unwrap();
    reject(
        &mut capture,
        &mut history,
        write.publish_at,
        REGION,
        schedule(VdpTimingError::UnqualifiedDisplayHistory),
    );
    let mut legacy = from_bank(
        configured(*Vdp::bootstrap_mode5().registers()),
        ROW_AT,
        false,
    );
    reject(
        &mut capture,
        &mut legacy,
        tick(ROW_AT),
        REGION,
        schedule(VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0x44,
        }),
    );
    let mut device = fixture();
    let mut pal = device.clone();
    let mut ntsc = device.clone();
    assert_eq!(
        capture(&mut pal, tick(ROW_AT + 1), Region::Pal),
        capture(&mut ntsc, tick(ROW_AT + 1), REGION)
    );
    assert_eq!(pal, ntsc);
    for at in [759_240, 261 * 3420 + 320] {
        reject(
            &mut capture,
            &mut device,
            tick(at),
            REGION,
            schedule(VdpTimingError::UnqualifiedMemoryInterval(tick(at))),
        );
    }
    reject(
        &mut capture,
        &mut device,
        tick(225 * 3420 + 1),
        REGION,
        render(VdpRenderError::CoordinateOutOfBounds { x: 0, y: 225 }),
    );
    let mut shadow = fixture();
    let write = shadow.queue_register(12, 8, REGION).unwrap();
    assert!(capture(&mut shadow, write.publish_at, REGION).is_ok());
    let mut boundary = fixture();
    let retained = boundary.prepare_line(REGION).unwrap();
    for at in [64 * 3420 + 2660, 65 * 3420 - 1, 65 * 3420] {
        let mut ordinary = boundary.clone();
        ordinary.advance_to(tick(at), REGION).unwrap();
        let row = capture(&mut boundary, tick(at), REGION).unwrap();
        assert_eq!(row, ordinary.prepare_line(REGION).unwrap());
        assert_eq!(boundary, ordinary);
        assert_eq!(row.row(), if at < 65 * 3420 { 64 } else { 65 });
        let colors = if at < 65 * 3420 {
            std::array::from_fn::<_, 256, _>(expected)
        } else {
            [color(15); 256]
        };
        assert_eq!(row.colors(), &colors);
    }
    assert_eq!(
        retained.colors(),
        &std::array::from_fn::<_, 256, _>(expected)
    );
    let mut last = late_fixture();
    last.advance_to(tick(u64::MAX - 1000), REGION).unwrap();
    enqueue(&mut last, 0x80, 1, 0x1234);
    for _ in 0..2 {
        reject(
            &mut capture,
            &mut last,
            tick(u64::MAX),
            REGION,
            render(VdpRenderError::UnsupportedWindowScroll),
        );
    }
    last.queue_register(17, 0, REGION).unwrap();
    let mut ordinary = last.clone();
    ordinary.advance_to(tick(u64::MAX), REGION).unwrap();
    let row = capture(&mut last, tick(u64::MAX), REGION).unwrap();
    assert_eq!((row.at(), row.row()), (tick(u64::MAX), 75));
    assert_eq!(row.colors(), &[color(15); 256]);
    assert_eq!(last, ordinary);
    assert_eq!(capture(&mut last, tick(u64::MAX), REGION).unwrap(), row);
}
