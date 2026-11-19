use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{VdpRegisterError, VdpRegisterPipeline, VdpRegisterWrite};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn publication_changes_only_at_the_deadline() {
    let mut unrelated = VdpRegisterPipeline::at_h32_edge(tick(0));
    unrelated.advance_to(tick(1)).unwrap();
    unrelated.queue_register(0, 0xff).unwrap();
    unrelated.advance_to(tick(20)).unwrap();
    assert_eq!(unrelated.auto_increment(), None);
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(31));
    assert_eq!(pipeline.auto_increment(), None);
    pipeline.advance_to(tick(32)).unwrap();
    let mut returned = pipeline.queue_register(15, 2).unwrap();
    returned.value = 255;
    assert_eq!(pipeline.auto_increment(), None);
    assert_eq!(pipeline.advance_to(tick(50)), Ok(None));
    assert_eq!(pipeline.auto_increment(), None);
    assert_eq!(pipeline.advance_to(tick(51)).unwrap().unwrap().value, 2);
    assert_eq!(pipeline.auto_increment(), Some(2));
    assert_eq!(returned.value, 255);
    assert_eq!(pipeline.advance_to(tick(51)), Ok(None));
    pipeline.advance_to(tick(52)).unwrap();
    pipeline.queue_register(15, 0).unwrap();
    assert_eq!(pipeline.auto_increment(), Some(2));
    assert_eq!(pipeline.advance_to(tick(70)), Ok(None));
    assert_eq!(pipeline.auto_increment(), Some(2));
    assert_eq!(
        pipeline.advance_to(tick(1000)).unwrap().unwrap().publish_at,
        tick(71)
    );
    assert_eq!(pipeline.auto_increment(), Some(0));
}

pub fn all_increment_values_survive_publication() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(1003));
    let mut previous = None;
    for value in 0u8..=255 {
        let at = 1004 + u64::from(value) * 20;
        let deadline = at + 19;
        assert_eq!(pipeline.advance_to(tick(at)), Ok(None));
        let expected = VdpRegisterWrite {
            index: 15,
            value,
            publish_at: tick(deadline),
        };
        assert_eq!(pipeline.queue_register(15, value), Ok(expected));
        assert_eq!(pipeline.auto_increment(), previous);
        assert_eq!(pipeline.advance_to(tick(deadline - 1)), Ok(None));
        assert_eq!(pipeline.auto_increment(), previous);
        assert_eq!(pipeline.advance_to(tick(deadline)), Ok(Some(expected)));
        assert_eq!(pipeline.auto_increment(), Some(value));
        assert_eq!(pipeline.advance_to(tick(deadline)), Ok(None));
        assert_eq!(pipeline.auto_increment(), Some(value));
        previous = Some(value);
    }
}

pub fn unrelated_requests_and_guards_preserve_increment() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(0));
    pipeline.advance_to(tick(1)).unwrap();
    pipeline.queue_register(15, 0xe7).unwrap();
    pipeline.advance_to(tick(20)).unwrap();
    for (round, (index, value)) in [(0, 0xff), (1, 4), (12, 8), (23, 0x80)]
        .into_iter()
        .enumerate()
    {
        let at = 21 + round as u64 * 20;
        pipeline.advance_to(tick(at)).unwrap();
        pipeline.queue_register(index, value).unwrap();
        let saved = pipeline;
        assert!(matches!(
            pipeline.queue_register(15, 0),
            Err(VdpRegisterError::Busy { .. })
        ));
        assert_eq!(pipeline, saved);
        assert_eq!(
            pipeline.advance_to(tick(at + 19)).unwrap().unwrap().index,
            index
        );
        assert_eq!(pipeline.auto_increment(), Some(0xe7));
    }
    let saved = pipeline;
    for (index, value, error) in [
        (24, 0, VdpRegisterError::UnsupportedRegister(24)),
        (
            1,
            0,
            VdpRegisterError::UnsupportedModeChange { index: 1, value: 0 },
        ),
        (
            12,
            0x80,
            VdpRegisterError::UnsupportedModeChange {
                index: 12,
                value: 0x80,
            },
        ),
        (15, 0, VdpRegisterError::CoincidentClockEdge),
    ] {
        assert_eq!(pipeline.queue_register(index, value), Err(error));
        assert_eq!(pipeline, saved);
    }
    assert!(matches!(
        pipeline.advance_to(tick(99)),
        Err(VdpRegisterError::TimeReversal { .. })
    ));
    assert_eq!(pipeline, saved);
    pipeline.advance_to(tick(u64::MAX)).unwrap();
    let saved = pipeline;
    assert_eq!(
        pipeline.queue_register(15, 0),
        Err(VdpRegisterError::ClockOverflow)
    );
    assert_eq!(pipeline, saved);
    assert_eq!(pipeline.auto_increment(), Some(0xe7));
}

pub fn cloned_pending_settings_and_chunk_invariance() {
    let mut pipeline = VdpRegisterPipeline::at_h32_edge(tick(31));
    pipeline.advance_to(tick(32)).unwrap();
    pipeline.queue_register(15, 1).unwrap();
    pipeline.advance_to(tick(51)).unwrap();
    pipeline.advance_to(tick(52)).unwrap();
    pipeline.queue_register(15, 0xff).unwrap();
    let saved = pipeline;
    let mut copied = pipeline;
    for at in [52, 60, 70] {
        assert_eq!(pipeline.advance_to(tick(at)), Ok(None));
        assert_eq!(pipeline.auto_increment(), Some(1));
    }
    let event = pipeline.advance_to(tick(71)).unwrap();
    assert_eq!(copied.advance_to(tick(1000)), Ok(event));
    pipeline.advance_to(tick(1000)).unwrap();
    assert_eq!(pipeline, copied);
    assert_eq!(pipeline.auto_increment(), Some(0xff));
    assert_eq!(saved.auto_increment(), Some(1));
    assert_eq!(saved.pending().unwrap().value, 0xff);
    copied.queue_register(15, 0).unwrap();
    copied.advance_to(tick(1011)).unwrap();
    assert_eq!(copied.auto_increment(), Some(0));
    assert_eq!(pipeline.auto_increment(), Some(0xff));
}
