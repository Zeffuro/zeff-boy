use super::support::base as support;

use support::*;
use zeff_md_core::{
    Region, Vdp, VdpInterruptError, VdpLineError, VdpRenderError, VdpScanline, VdpScheduleError,
    VdpTimingError, VdpVerticalTiming,
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

fn pointers(device: &Vdp) -> (*const u8, *const u16, *const u16) {
    (
        device.memory().vram().as_ptr(),
        device.memory().cram().as_ptr(),
        device.memory().vsram().as_ptr(),
    )
}

fn reject(
    snapshot: &mut impl FnMut(&Vdp, u16, Region) -> Snapshot,
    device: &Vdp,
    row: u16,
    region: Region,
    error: VdpLineError,
) {
    let saved = device.clone();
    let addresses = pointers(device);
    assert_eq!(snapshot(device, row, region), Err(error));
    assert_eq!(*device, saved);
    assert_eq!(pointers(device), addresses);
}

pub fn coordinates_and_sprite_guards() {
    coordinates_and_sprite_guards_with(Vdp::prepare_logical_line);
}

pub fn coordinates_and_sprite_guards_with(mut snapshot: impl FnMut(&Vdp, u16, Region) -> Snapshot) {
    let device = fixture();
    let saved = device.clone();
    let addresses = pointers(&device);
    assert_eq!(
        snapshot(&device, ROW, REGION).unwrap().colors(),
        &std::array::from_fn::<_, 256, _>(expected)
    );
    assert_eq!(device, saved);
    assert_eq!(pointers(&device), addresses);
    for row in [224, u16::MAX] {
        reject(
            &mut snapshot,
            &device,
            row,
            REGION,
            render(VdpRenderError::CoordinateOutOfBounds { x: 0, y: row }),
        );
    }
    let cold = device_at(ROW_AT);
    reject(
        &mut snapshot,
        &cold,
        ROW,
        REGION,
        render(VdpRenderError::UnknownSpriteCache(0)),
    );
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
            assert_eq!(snapshot(&device, ROW, REGION).unwrap().colors(), &[0; 256]);
        } else {
            reject(
                &mut snapshot,
                &device,
                ROW,
                REGION,
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
        ROW,
        REGION,
        render(VdpRenderError::UnsupportedSpriteRow),
    );
    for (address, word, error) in [
        (0x2006, 0, VdpRenderError::UnsupportedSpriteMask),
        (0x200a, 1, VdpRenderError::SpriteLinkCycle(1)),
        (0x200a, 2, VdpRenderError::UnknownSpriteCache(2)),
    ] {
        let mut device = fixture();
        words(&mut device, address, 1, &[word]);
        reject(&mut snapshot, &device, ROW, REGION, render(error));
    }
}

pub fn pending_and_render_guards() {
    pending_and_render_guards_with(Vdp::prepare_logical_line);
}

pub fn pending_and_render_guards_with(mut snapshot: impl FnMut(&Vdp, u16, Region) -> Snapshot) {
    let mut pending = fixture();
    command(&mut pending, 0, 3);
    pending.write_data(0xeeee, REGION).unwrap();
    assert!(!pending.fifo().is_empty());
    reject(
        &mut snapshot,
        &pending,
        ROW,
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    drain(&mut pending);
    assert!(snapshot(&pending, ROW, REGION).is_ok());
    command(&mut pending, 0, 0);
    reject(
        &mut snapshot,
        &pending,
        ROW,
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    let first = VdpVerticalTiming::at_counters_zero(tick(0))
        .next_memory_slot(pending.now(), REGION, pending.registers())
        .unwrap();
    pending.advance_to(first, REGION).unwrap();
    assert!(!pending.read_cache().is_ready());
    reject(
        &mut snapshot,
        &pending,
        ROW,
        REGION,
        render(VdpRenderError::PendingLineInputs),
    );
    let ready = pending.data_read_wait(REGION).unwrap().unwrap();
    pending.advance_to(tick(ready.get() + 1), REGION).unwrap();
    assert!(pending.read_cache().is_ready());
    let saved = pending.clone();
    let addresses = pointers(&pending);
    assert!(snapshot(&pending, ROW, REGION).is_ok());
    assert_eq!(pending, saved);
    assert_eq!(pointers(&pending), addresses);
    assert!(pending.read_cache().is_ready());
    let mut shadow = fixture();
    register(&mut shadow, 12, 8);
    assert!(snapshot(&shadow, ROW, REGION).is_ok());
    let mut scroll = fixture();
    register(&mut scroll, 11, 1);
    reject(
        &mut snapshot,
        &scroll,
        ROW,
        REGION,
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
    assert_eq!(
        late.memory().sample_background(late.registers(), 16, ROW),
        Err(VdpRenderError::UnsupportedWindowScroll)
    );
    reject(
        &mut snapshot,
        &late,
        ROW,
        REGION,
        render(VdpRenderError::UnsupportedWindowScroll),
    );
}

pub fn configuration_guards() {
    configuration_guards_with(Vdp::prepare_logical_line);
}

pub fn configuration_guards_with(mut snapshot: impl FnMut(&Vdp, u16, Region) -> Snapshot) {
    reject(
        &mut snapshot,
        &Vdp::bootstrap_mode5(),
        ROW,
        REGION,
        schedule(VdpTimingError::UnsupportedMode { index: 1, value: 4 }),
    );
    let device = fixture();
    let saved = device.clone();
    assert_eq!(
        snapshot(&device, ROW, Region::Pal),
        snapshot(&device, ROW, REGION)
    );
    assert_eq!(device, saved);
    for index in [1, 12] {
        let mut pending = fixture();
        pending
            .queue_register(index, if index == 1 { 0x04 } else { 0x10 }, REGION)
            .unwrap();
        reject(
            &mut snapshot,
            &pending,
            ROW,
            REGION,
            schedule(VdpTimingError::PendingModeChange),
        );
    }
    let mut history = fixture();
    register(&mut history, 1, 4);
    reject(
        &mut snapshot,
        &history,
        ROW,
        REGION,
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
        ROW,
        REGION,
        schedule(VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0x44,
        }),
    );
    let mut extended = fixture();
    register(&mut extended, 1, 0xc4);
    reject(
        &mut snapshot,
        &extended,
        ROW,
        REGION,
        schedule(VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0xc4,
        }),
    );
}
