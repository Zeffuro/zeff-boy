use super::support::*;
use zeff_md_core::{
    Region, Vdp, VdpInterruptError, VdpLineError, VdpRegisterPipeline, VdpRenderError, VdpScanline,
    VdpScheduleError, VdpTimingError,
};

type Snapshot = Result<VdpScanline, VdpLineError>;

fn render(error: VdpRenderError) -> VdpLineError {
    VdpLineError::Render(error)
}

fn schedule(error: VdpTimingError) -> VdpLineError {
    VdpLineError::Schedule(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
        error,
    )))
}

fn reject(snapshot: &mut impl FnMut(&Vdp, Region) -> Snapshot, device: &Vdp, error: VdpLineError) {
    let saved = device.clone();
    assert_eq!(snapshot(device, REGION), Err(error));
    assert_eq!(*device, saved);
}

pub fn asymmetric_canonical_row() {
    asymmetric_canonical_row_with(Vdp::prepare_line);
}

pub fn asymmetric_canonical_row_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    let mut device = fixture();
    let saved = device.clone();
    let row = snapshot(&device, REGION).unwrap();
    assert_eq!(snapshot(&device, Region::NtscJapan).unwrap(), row);
    assert_eq!(row.at(), tick(ROW_AT));
    assert_eq!(row.row(), ROW);
    assert_eq!(device, saved);
    let cache = device.sprite_cache().unwrap();
    let selected = cache.select_row(ROW).unwrap();
    let patterns = [
        selected
            .prepare_sprite(cache, device.memory(), device.registers(), 0)
            .unwrap()
            .unwrap(),
        selected
            .prepare_sprite(cache, device.memory(), device.registers(), 1)
            .unwrap()
            .unwrap(),
    ];
    let sprites = device.memory().compose_sprite_line(&patterns).unwrap();
    for x in 0..256 {
        let literal = expected(x);
        assert_eq!(row.colors()[x], literal, "x={x}");
        assert_eq!(
            device
                .memory()
                .sample_composite(device.registers(), x as u16, ROW),
            Ok(literal)
        );
        assert_eq!(
            device
                .memory()
                .sample_sprite_line(device.registers(), &sprites, x as u16, ROW),
            Ok(literal)
        );
    }
    register(&mut device, 17, 0x80);
    let window = snapshot(&device, REGION).unwrap();
    for x in 0..256 {
        let literal = if x < 8 {
            color(x as u16 + 1)
        } else if x < 24 {
            color(18)
        } else if x < 32 {
            expected(x)
        } else {
            color(15)
        };
        assert_eq!(window.colors()[x], literal, "window x={x}");
    }
}

pub fn owned_snapshot_and_replay() {
    owned_snapshot_and_replay_with(Vdp::prepare_line);
}

pub fn owned_snapshot_and_replay_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    let mut device = fixture();
    let mut replay = device.clone();
    let row = snapshot(&device, REGION).unwrap();
    let copy = row.clone();
    assert_eq!(snapshot(&replay, REGION).unwrap(), row);
    for target in [&mut device, &mut replay] {
        register(target, 7, 14);
        words(target, 15 * 2, 3, &[0xeeee]);
        words(target, 0xc208, 1, &[1]);
    }
    assert_eq!(device, replay);
    let next = snapshot(&device, REGION).unwrap();
    assert_eq!(snapshot(&replay, REGION).unwrap(), next);
    assert_ne!(next, row);
    assert_eq!(row, copy);
    assert_eq!(row.at(), tick(ROW_AT));
    assert_eq!(row.colors(), &std::array::from_fn::<_, 256, _>(expected));
    assert_eq!(next.colors()[255], color(14));
    assert_eq!(device.memory().cram()[15], 0x0eee);
}

pub fn publication_and_cached_layout() {
    publication_and_cached_layout_with(Vdp::prepare_line);
}

pub fn publication_and_cached_layout_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    let mut device = fixture();
    let write = device.queue_register(7, 14, REGION).unwrap();
    for at in [
        write.publish_at.get() - 1,
        write.publish_at.get(),
        write.publish_at.get() + 1,
    ] {
        device.advance_to(tick(at), REGION).unwrap();
        let row = snapshot(&device, REGION).unwrap();
        assert_eq!(
            row.colors()[255],
            color(if at < write.publish_at.get() { 15 } else { 14 })
        );
    }
    let cached = *device.sprite_cache().unwrap();
    let write = device.queue_register(5, 0x20, REGION).unwrap();
    for at in [
        write.publish_at.get() - 1,
        write.publish_at.get(),
        write.publish_at.get() + 1,
    ] {
        device.advance_to(tick(at), REGION).unwrap();
        let row = snapshot(&device, REGION).unwrap();
        assert_eq!(*device.sprite_cache().unwrap(), cached);
        assert_eq!(
            row.colors()[40],
            if at < write.publish_at.get() {
                color(14)
            } else {
                color(52)
            }
        );
        assert_eq!(
            row.colors()[48],
            if at < write.publish_at.get() {
                color(14)
            } else {
                color(52)
            }
        );
        let entry = device
            .sprite_cache()
            .unwrap()
            .sprite_entry(device.memory(), device.registers(), 0)
            .unwrap();
        assert_eq!(
            (entry.y, entry.width, entry.height, entry.link),
            (64, 8, 8, 1)
        );
        assert_eq!(entry.x, if at < write.publish_at.get() { 0 } else { 40 });
    }
}

pub fn complete_sprite_list_limits() {
    complete_sprite_list_limits_with(Vdp::prepare_line);
}

pub fn complete_sprite_list_limits_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    for count in [16u8, 17] {
        let mut device = device_at(301);
        for index in 0..count {
            sat(
                &mut device,
                0x2000,
                index,
                [
                    192,
                    if index + 1 == count {
                        0
                    } else {
                        u16::from(index + 1)
                    },
                    0,
                    129,
                ],
            );
        }
        device.advance_to(tick(ROW_AT), REGION).unwrap();
        assert_eq!(
            device
                .sprite_cache()
                .unwrap()
                .select_row(ROW)
                .unwrap()
                .indices()
                .len(),
            16
        );
        if count == 16 {
            assert_eq!(snapshot(&device, REGION).unwrap().colors(), &[0; 256]);
        } else {
            reject(
                &mut snapshot,
                &device,
                render(VdpRenderError::UnsupportedSpriteRow),
            );
        }
    }
    let mut width = device_at(301);
    for index in 0..9 {
        sat(
            &mut width,
            0x2000,
            index,
            [
                192,
                0xc00 | if index == 8 { 0 } else { u16::from(index + 1) },
                0,
                129,
            ],
        );
    }
    width.advance_to(tick(ROW_AT), REGION).unwrap();
    reject(
        &mut snapshot,
        &width,
        render(VdpRenderError::UnsupportedSpriteRow),
    );
    let mut masked = fixture();
    words(&mut masked, 0x2006, 1, &[0]);
    reject(
        &mut snapshot,
        &masked,
        render(VdpRenderError::UnsupportedSpriteMask),
    );
    let mut cycle = fixture();
    words(&mut cycle, 0x200a, 1, &[1]);
    reject(
        &mut snapshot,
        &cycle,
        render(VdpRenderError::SpriteLinkCycle(1)),
    );
    let mut unknown = fixture();
    words(&mut unknown, 0x200a, 1, &[2]);
    reject(
        &mut snapshot,
        &unknown,
        render(VdpRenderError::UnknownSpriteCache(2)),
    );
}

pub fn pending_inputs_and_render_guards() {
    pending_inputs_and_render_guards_with(Vdp::prepare_line);
}

pub fn pending_inputs_and_render_guards_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    let cold = device_at(ROW_AT);
    reject(
        &mut snapshot,
        &cold,
        render(VdpRenderError::UnknownSpriteCache(0)),
    );
    let mut empty = device_at(301);
    palette(&mut empty);
    sat(&mut empty, 0x2000, 0, [0, 0, 0, 128]);
    empty.advance_to(tick(ROW_AT), REGION).unwrap();
    assert!(
        empty
            .sprite_cache()
            .unwrap()
            .select_row(ROW)
            .unwrap()
            .indices()
            .is_empty()
    );
    assert_eq!(
        snapshot(&empty, REGION).unwrap().colors(),
        &[color(15); 256]
    );
    let mut pending = fixture();
    command(&mut pending, 0, 3);
    pending.write_data(0xeeee, REGION).unwrap();
    reject(
        &mut snapshot,
        &pending,
        render(VdpRenderError::PendingLineInputs),
    );
    drain(&mut pending);
    assert!(snapshot(&pending, REGION).is_ok());
    command(&mut pending, 0, 0);
    reject(
        &mut snapshot,
        &pending,
        render(VdpRenderError::PendingLineInputs),
    );
    let first = zeff_md_core::VdpVerticalTiming::at_counters_zero(tick(0))
        .next_memory_slot(pending.now(), REGION, pending.registers())
        .unwrap();
    pending.advance_to(first, REGION).unwrap();
    assert!(!pending.read_cache().is_ready());
    reject(
        &mut snapshot,
        &pending,
        render(VdpRenderError::PendingLineInputs),
    );
    let ready = pending.data_read_wait(REGION).unwrap().unwrap();
    pending.advance_to(tick(ready.get() + 1), REGION).unwrap();
    assert!(snapshot(&pending, REGION).is_ok());
    let mut shadow = fixture();
    register(&mut shadow, 12, 8);
    assert!(snapshot(&shadow, REGION).is_ok());
    let mut scroll = fixture();
    register(&mut scroll, 11, 1);
    reject(
        &mut snapshot,
        &scroll,
        render(VdpRenderError::UnsupportedScrollMode(1)),
    );
    let mut late = fixture();
    register(&mut late, 17, 1);
    words(&mut late, 0xf000, 1, &[1]);
    assert!(
        late.memory()
            .sample_background(late.registers(), 15, ROW)
            .is_ok()
    );
    reject(
        &mut snapshot,
        &late,
        render(VdpRenderError::UnsupportedWindowScroll),
    );
    let mut bank = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [
        (0, 0),
        (1, 0x44),
        (2, 0x30),
        (3, 0x1c),
        (4, 5),
        (5, 0x10),
        (7, 15),
        (10, 0xff),
        (12, 0),
        (13, 0x3c),
        (15, 2),
        (16, 0),
        (17, 0),
        (18, 0),
    ] {
        publish(&mut bank, index, value);
    }
    let mut unknown = from_bank(bank, 301, true);
    sat(&mut unknown, 0x2000, 0, [0, 0, 0, 128]);
    unknown.advance_to(tick(ROW_AT), REGION).unwrap();
    reject(
        &mut snapshot,
        &unknown,
        render(VdpRenderError::UnknownRegister(11)),
    );
}

pub fn logical_row_and_schedule_guards() {
    logical_row_and_schedule_guards_with(Vdp::prepare_line);
}

pub fn logical_row_and_schedule_guards_with(mut snapshot: impl FnMut(&Vdp, Region) -> Snapshot) {
    reject(
        &mut snapshot,
        &Vdp::bootstrap_mode5(),
        schedule(VdpTimingError::UnsupportedMode { index: 1, value: 4 }),
    );
    let mut device = fixture();
    for at in [
        64 * 3420 + 2659,
        64 * 3420 + 2660,
        64 * 3420 + 2661,
        65 * 3420 - 1,
        65 * 3420,
        65 * 3420 + 1,
    ] {
        device.advance_to(tick(at), REGION).unwrap();
        assert_eq!(
            snapshot(&device, REGION).unwrap().row(),
            if at < 65 * 3420 { 64 } else { 65 }
        );
    }
    let bank = configured(VdpRegisterPipeline::at_h32_edge(tick(20)));
    let mut offset = from_bank(bank, 341, true);
    sat(&mut offset, 0x2000, 0, [0, 0, 0, 128]);
    offset.advance_to(tick(65 * 3420 + 1), REGION).unwrap();
    assert_eq!(snapshot(&offset, REGION).unwrap().row(), 65);
    for index in [1, 12] {
        let mut pending = fixture();
        pending
            .queue_register(index, if index == 1 { 0x04 } else { 0x10 }, REGION)
            .unwrap();
        reject(
            &mut snapshot,
            &pending,
            schedule(VdpTimingError::PendingModeChange),
        );
    }
    let mut history = fixture();
    register(&mut history, 1, 4);
    reject(
        &mut snapshot,
        &history,
        schedule(VdpTimingError::UnqualifiedDisplayHistory),
    );
    let legacy = from_bank(
        configured(*Vdp::bootstrap_mode5().registers()),
        ROW_AT,
        false,
    );
    reject(
        &mut snapshot,
        &legacy,
        schedule(VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0x44,
        }),
    );
    let mut margins = fixture();
    margins.advance_to(tick(759240), REGION).unwrap();
    reject(
        &mut snapshot,
        &margins,
        schedule(VdpTimingError::UnqualifiedMemoryInterval(tick(759240))),
    );
    margins.advance_to(tick(225 * 3420 + 1), REGION).unwrap();
    reject(
        &mut snapshot,
        &margins,
        render(VdpRenderError::CoordinateOutOfBounds { x: 0, y: 225 }),
    );
    let saved = device.clone();
    assert_eq!(snapshot(&device, Region::Pal), snapshot(&device, REGION));
    assert_eq!(device, saved);
}
