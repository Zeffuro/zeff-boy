use super::support::*;
use zeff_md_core::{Region, Vdp, VdpLineError, VdpRenderError, VdpScanline};

type Line = Result<VdpScanline, VdpLineError>;

pub fn all_logical_rows() {
    all_logical_rows_with(Vdp::prepare_logical_line);
}

pub fn all_logical_rows_with(mut prepare: impl FnMut(&Vdp, u16, Region) -> Line) {
    let device = fixture();
    let saved = device.clone();
    let pointer = device.memory().vram().as_ptr();
    for row in 0..224 {
        let line = prepare(&device, row, REGION).unwrap();
        assert_eq!(line.at(), device.now());
        assert_eq!(line.row(), row);
        for x in 0..256 {
            assert_eq!(line.colors()[x], expected(row, x), "row{row} x{x}");
        }
        assert_eq!(device, saved);
        assert_eq!(device.memory().vram().as_ptr(), pointer);
    }
    let first = prepare(&device, 0, REGION).unwrap();
    let last = prepare(&device, 223, REGION).unwrap();
    let mut later = device.clone();
    register(&mut later, 7, 14);
    words(&mut later, 30, 3, &[0x0246]);
    assert_eq!(prepare(&device, 0, Region::NtscJapan), Ok(first.clone()));
    assert_eq!(prepare(&saved, 223, REGION), Ok(last.clone()));
    assert_eq!(first.colors()[255], color(15));
    assert_eq!(last.colors()[255], color(15));
    assert_eq!(prepare(&later, 0, REGION).unwrap().colors()[255], color(14));
    assert_eq!(
        prepare(&later, 223, REGION).unwrap().colors()[255],
        color(14)
    );
}

pub fn static_rows_and_timed_guards() {
    static_rows_and_timed_guards_with(Vdp::prepare_logical_line);
}

pub fn static_rows_and_timed_guards_with(mut prepare: impl FnMut(&Vdp, u16, Region) -> Line) {
    let mut device = fixture();
    assert!(device.now().get() > ROW_AT);
    for cycle in [222, 223, 224, 225, 259, 261, 262] {
        device.advance_to(tick(cycle * 3420 + 1), REGION).unwrap();
        let saved = device.clone();
        let first = prepare(&device, 0, REGION).unwrap();
        let last = prepare(&device, 223, REGION).unwrap();
        assert_eq!(first.at(), device.now());
        assert_eq!(last.at(), device.now());
        assert_eq!(
            first.colors(),
            &std::array::from_fn::<_, 256, _>(|x| expected(0, x))
        );
        assert_eq!(
            last.colors(),
            &std::array::from_fn::<_, 256, _>(|x| expected(223, x))
        );
        assert_eq!(device, saved);
        if [222, 223, 224, 259, 261].contains(&cycle) {
            let error = VdpLineError::Schedule(zeff_md_core::VdpScheduleError::Interrupt(
                zeff_md_core::VdpInterruptError::Timing(
                    zeff_md_core::VdpTimingError::UnqualifiedMemoryInterval(device.now()),
                ),
            ));
            assert_eq!(device.prepare_line(REGION), Err(error));
            let at = device.now();
            assert_eq!(device.capture_line_at(at, REGION), Err(error));
            assert_eq!(device, saved);
            let mut ordinary = device.clone();
            ordinary.write_data(0x1234, REGION).unwrap();
            ordinary
                .advance_to(tick(ordinary.now().get() + 1000), REGION)
                .unwrap();
            assert!(ordinary.fifo().is_empty());
        } else if cycle == 225 {
            assert_eq!(
                device.prepare_line(REGION),
                Err(VdpLineError::Render(
                    VdpRenderError::CoordinateOutOfBounds { x: 0, y: 225 }
                ))
            );
        } else {
            assert_eq!(device.prepare_line(REGION), Ok(first));
        }
    }
}
