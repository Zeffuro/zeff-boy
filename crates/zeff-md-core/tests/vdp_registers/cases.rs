use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlWrite, VdpControl, VdpRegisterError, VdpRegisterPipeline, VdpRegisterWrite,
};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn write(index: u8, value: u8, at: u64) -> VdpRegisterWrite {
    VdpRegisterWrite {
        index,
        value,
        publish_at: tick(at),
    }
}

pub fn accepted(index: u8, value: u8) -> bool {
    (index != 1 || value & 4 != 0) && (index != 12 || matches!(value & 0x81, 0 | 0x81))
}

pub fn known_phase_deadlines_and_boundaries() {
    for epoch in [0, 1003, 1 << 32] {
        for period in [0, 3] {
            for phase in 1..20 {
                for (index, value) in [(0, 0x55), (1, 0x74), (12, 8), (15, 2), (23, 0xe0)] {
                    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(epoch));
                    let at = epoch + period * 20 + phase;
                    let deadline = epoch + (period + 1) * 20;
                    assert_eq!(pipeline.advance_to(tick(at)), Ok(None));
                    let expected = write(index, value, deadline);
                    assert_eq!(pipeline.queue_register(index, value), Ok(expected));
                    assert_eq!(pipeline.pending(), Some(expected));
                    assert_eq!(pipeline.advance_to(tick(deadline - 1)), Ok(None));
                    assert_eq!(pipeline.pending(), Some(expected));
                    assert_eq!(pipeline.advance_to(tick(deadline)), Ok(Some(expected)));
                    assert_eq!(pipeline.pending(), None);
                    assert_eq!(pipeline.advance_to(tick(deadline)), Ok(None));
                    assert_eq!(pipeline.advance_to(tick(deadline + 1)), Ok(None));
                }
            }
        }
    }
}

pub fn register_payload_partition() {
    let mut allowed = 0;
    let mut rejected = 0;
    for index in 0..24 {
        for value in 0u8..=255 {
            let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(1003));
            pipeline.advance_to(tick(1004)).unwrap();
            let saved = pipeline;
            if accepted(index, value) {
                let expected = write(index, value, 1023);
                assert_eq!(pipeline.queue_register(index, value), Ok(expected));
                assert_eq!(pipeline.advance_to(tick(1023)), Ok(Some(expected)));
                assert_eq!(pipeline.pending(), None);
                allowed += 1;
            } else {
                assert_eq!(
                    pipeline.queue_register(index, value),
                    Err(VdpRegisterError::UnsupportedModeChange { index, value })
                );
                assert_eq!(pipeline, saved);
                rejected += 1;
            }
        }
    }
    assert_eq!((allowed, rejected), (5888, 256));
}

pub fn copied_payload_and_chunk_invariance() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(31));
    pipeline.advance_to(tick(32)).unwrap();
    let expected = write(15, 2, 51);
    let mut returned = pipeline.queue_register(15, 2).unwrap();
    assert_eq!(returned, expected);
    returned.index = 1;
    returned.value = 0;
    returned.publish_at = tick(u64::MAX);
    assert_ne!(returned, expected);
    assert_eq!(pipeline.pending(), Some(expected));
    let mut copied = pipeline;
    let mut fine_result = None;
    for at in [32, 49, 50, 51, 51, 1000] {
        if let Some(event) = pipeline.advance_to(tick(at)).unwrap() {
            assert!(fine_result.replace(event).is_none());
        }
    }
    assert_eq!(fine_result, Some(expected));
    assert_eq!(copied.advance_to(tick(1000)), Ok(Some(expected)));
    assert_eq!(pipeline, copied);
    assert_eq!(copied.advance_to(tick(1000)), Ok(None));
    assert_eq!(copied.now(), tick(1000));
    let next = write(0, 0x55, 1011);
    assert_eq!(copied.queue_register(0, 0x55), Ok(next));
    assert_eq!(copied.advance_to(tick(1011)), Ok(Some(next)));
    let saved = copied;
    assert_eq!(
        copied.queue_register(0, 0x55),
        Err(VdpRegisterError::CoincidentClockEdge)
    );
    assert_eq!(copied, saved);
}

pub fn busy_and_invalid_requests_are_atomic() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(0));
    for edge in [0, 20, 40] {
        pipeline.advance_to(tick(edge)).unwrap();
        let saved = pipeline;
        assert_eq!(
            pipeline.queue_register(15, 2),
            Err(VdpRegisterError::CoincidentClockEdge)
        );
        assert_eq!(pipeline, saved);
    }
    pipeline.advance_to(tick(41)).unwrap();
    let expected = write(15, 2, 60);
    pipeline.queue_register(15, 2).unwrap();
    let saved = pipeline;
    assert_eq!(
        pipeline.queue_register(0, 0xff),
        Err(VdpRegisterError::Busy {
            publish_at: tick(60)
        })
    );
    assert_eq!(pipeline, saved);
    for index in 24u8..=255 {
        assert_eq!(
            pipeline.queue_register(index, 0xff),
            Err(VdpRegisterError::UnsupportedRegister(index))
        );
        assert_eq!(pipeline, saved);
    }
    for (index, value) in [(1, 0), (1, 0xfb), (12, 1), (12, 0x80)] {
        assert_eq!(
            pipeline.queue_register(index, value),
            Err(VdpRegisterError::UnsupportedModeChange { index, value })
        );
        assert_eq!(pipeline, saved);
    }
    assert_eq!(pipeline.advance_to(tick(61)), Ok(Some(expected)));
    assert_eq!(pipeline.queue_register(1, 4), Ok(write(1, 4, 80)));
    assert_eq!(pipeline.advance_to(tick(80)), Ok(Some(write(1, 4, 80))));
}

pub fn reverse_time_and_overflow_are_atomic() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(31));
    pipeline.advance_to(tick(32)).unwrap();
    pipeline.queue_register(15, 2).unwrap();
    pipeline.advance_to(tick(50)).unwrap();
    let saved = pipeline;
    assert_eq!(
        pipeline.advance_to(tick(49)),
        Err(VdpRegisterError::TimeReversal {
            current: tick(50),
            requested: tick(49),
        })
    );
    assert_eq!(pipeline, saved);
    assert_eq!(pipeline.advance_to(tick(51)), Ok(Some(write(15, 2, 51))));
    for (epoch, now) in [(u64::MAX - 19, u64::MAX - 18), (0, u64::MAX)] {
        let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(epoch));
        pipeline.advance_to(tick(now)).unwrap();
        let saved = pipeline;
        assert_eq!(
            pipeline.queue_register(0, 0xff),
            Err(VdpRegisterError::ClockOverflow)
        );
        assert_eq!(pipeline, saved);
    }
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(u64::MAX - 20));
    pipeline.advance_to(tick(u64::MAX - 19)).unwrap();
    let expected = write(0, 0xff, u64::MAX);
    assert_eq!(pipeline.queue_register(0, 0xff), Ok(expected));
    assert_eq!(pipeline.advance_to(tick(u64::MAX - 1)), Ok(None));
    assert_eq!(pipeline.advance_to(tick(u64::MAX)), Ok(Some(expected)));
    assert_eq!(pipeline.advance_to(tick(u64::MAX)), Ok(None));
}

pub fn decoder_requests_wait_for_publication() {
    let mut control = VdpControl::default();
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(1003));
    pipeline.advance_to(tick(1004)).unwrap();
    let ControlWrite::Register { index, value } = control.write_control(0x8f02) else {
        panic!("register request")
    };
    let expected = write(15, 2, 1023);
    assert_eq!(pipeline.queue_register(index, value), Ok(expected));
    control.write_control(0x4567);
    assert!(matches!(
        control.write_control(0x8f02),
        ControlWrite::Command(_)
    ));
    assert_eq!(pipeline.pending(), Some(expected));
    assert_eq!(pipeline.advance_to(tick(1022)), Ok(None));
    let mut copied = pipeline;
    assert_eq!(pipeline.advance_to(tick(1023)), Ok(Some(expected)));
    assert_eq!(copied.advance_to(tick(1023)), Ok(Some(expected)));
    assert_eq!(pipeline, copied);
}
