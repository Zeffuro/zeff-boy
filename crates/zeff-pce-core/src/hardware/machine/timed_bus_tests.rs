use super::*;

#[test]
fn deferred_faults_credit_only_the_completed_device_chunks() {
    fn run(coalesce: bool) -> (PceMachineError, u64, u64, u64, u16) {
        let mut inner = BaseBus::new(Vec::new(), PceDevices::default()).unwrap();
        let mut front_video = PceActiveOnlyVideoFrame::new();
        let mut back_video = PceActiveOnlyVideoFrame::new();
        let mut vce_line_accumulator = 0;
        let mut vdc_pixel_clock_remainder = 0;
        let mut vce_line_index = 0;
        let mut vce_frame_length = VceFrameLength::Lines262;
        let mut debug = AddressDebugController::new();
        #[cfg(feature = "profiling")]
        let mut profiling = PceProfiling::default();
        let (fault, elapsed, on_chip_ticks) = {
            let mut bus = TimedMachineBus::new(
                &mut inner,
                &mut front_video,
                &mut back_video,
                &mut vce_line_accumulator,
                &mut vdc_pixel_clock_remainder,
                &mut vce_line_index,
                &mut vce_frame_length,
                1,
                None,
                &mut debug,
                coalesce,
                #[cfg(feature = "profiling")]
                &mut profiling,
            );
            bus.fault_after_device_chunks = Some(1);
            bus.advance_elapsed_master_ticks(2 * PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE);
            bus.finish_action();
            (
                bus.fault.expect("injected device fault"),
                bus.elapsed_master_ticks,
                bus.take_elapsed_master_ticks(),
            )
        };
        (
            fault,
            elapsed,
            on_chip_ticks,
            vce_line_accumulator,
            vce_line_index,
        )
    }

    let scalar = run(false);
    let coalesced = run(true);
    assert_eq!(coalesced, scalar);
    assert_eq!(coalesced.1, PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE);
    assert_eq!(coalesced.2, PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE);
}

#[test]
fn deferred_fault_horizon_stops_an_ordinary_write_before_it_executes() {
    for master_ticks_per_cycle in [
        PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    ] {
        for coalesce in [false, true] {
            let mut inner = BaseBus::new(Vec::new(), PceDevices::default()).unwrap();
            let mut front_video = PceActiveOnlyVideoFrame::new();
            let mut back_video = PceActiveOnlyVideoFrame::new();
            let mut vce_line_accumulator = 0;
            let mut vdc_pixel_clock_remainder = 0;
            let mut vce_line_index = 0;
            let mut vce_frame_length = VceFrameLength::Lines262;
            let mut debug = AddressDebugController::new();
            #[cfg(feature = "profiling")]
            let mut profiling = PceProfiling::default();
            {
                let mut bus = TimedMachineBus::new(
                    &mut inner,
                    &mut front_video,
                    &mut back_video,
                    &mut vce_line_accumulator,
                    &mut vdc_pixel_clock_remainder,
                    &mut vce_line_index,
                    &mut vce_frame_length,
                    master_ticks_per_cycle,
                    None,
                    &mut debug,
                    coalesce,
                    #[cfg(feature = "profiling")]
                    &mut profiling,
                );
                bus.fault_after_device_chunks = Some(1);
                bus.write(0x1F_0000, 0x5A);
                assert!(bus.fault.is_some());
                assert_eq!(bus.elapsed_master_ticks, master_ticks_per_cycle);
            }
            assert_eq!(inner.peek(0x1F_0000), 0);
        }
    }
}

#[test]
fn ordinary_accesses_and_idle_coalesce_until_an_action_boundary() {
    fn calls_after_finish(coalesce: bool) -> (u64, u64, u64) {
        let mut inner = BaseBus::new(Vec::new(), PceDevices::default()).unwrap();
        let mut front_video = PceActiveOnlyVideoFrame::new();
        let mut back_video = PceActiveOnlyVideoFrame::new();
        let mut vce_line_accumulator = 0;
        let mut vdc_pixel_clock_remainder = 0;
        let mut vce_line_index = 0;
        let mut vce_frame_length = VceFrameLength::Lines262;
        let mut debug = AddressDebugController::new();
        #[cfg(feature = "profiling")]
        let mut profiling = PceProfiling::default();
        let mut bus = TimedMachineBus::new(
            &mut inner,
            &mut front_video,
            &mut back_video,
            &mut vce_line_accumulator,
            &mut vdc_pixel_clock_remainder,
            &mut vce_line_index,
            &mut vce_frame_length,
            1,
            None,
            &mut debug,
            coalesce,
            #[cfg(feature = "profiling")]
            &mut profiling,
        );
        assert_eq!(bus.read(0), 0xFF);
        assert_eq!(bus.dummy_read(1), 0xFF);
        bus.idle();
        let claimed_before_finish = bus.take_elapsed_master_ticks();
        assert_eq!(claimed_before_finish, if coalesce { 0 } else { 3 });
        bus.finish_action();
        (
            bus.device_advance_calls,
            bus.elapsed_master_ticks,
            claimed_before_finish + bus.take_elapsed_master_ticks(),
        )
    }

    assert_eq!(calls_after_finish(false), (3, 3, 3));
    assert_eq!(calls_after_finish(true), (1, 3, 3));
}

#[test]
fn ordinary_access_at_the_vce_line_horizon_materializes_before_the_write() {
    let mut inner = BaseBus::new(Vec::new(), PceDevices::default()).unwrap();
    let mut front_video = PceActiveOnlyVideoFrame::new();
    let mut back_video = PceActiveOnlyVideoFrame::new();
    let mut vce_line_accumulator = PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - 2;
    let mut vdc_pixel_clock_remainder = 0;
    let mut vce_line_index = 0;
    let mut vce_frame_length = VceFrameLength::Lines262;
    let mut debug = AddressDebugController::new();
    #[cfg(feature = "profiling")]
    let mut profiling = PceProfiling::default();
    let mut bus = TimedMachineBus::new(
        &mut inner,
        &mut front_video,
        &mut back_video,
        &mut vce_line_accumulator,
        &mut vdc_pixel_clock_remainder,
        &mut vce_line_index,
        &mut vce_frame_length,
        1,
        None,
        &mut debug,
        true,
        #[cfg(feature = "profiling")]
        &mut profiling,
    );

    bus.write(0x1F_0000, 0x11);
    assert_eq!(bus.pending_device_master_ticks, 1);
    assert_eq!(bus.device_advance_calls, 0);
    assert_eq!(bus.inner.peek(0x1F_0000), 0x11);

    bus.write(0x1F_0000, 0x22);
    assert_eq!(bus.pending_device_master_ticks, 0);
    assert_eq!(bus.device_advance_calls, 1);
    assert_eq!(*bus.vce_line_accumulator, 0);
    assert_eq!(*bus.vce_line_index, 1);
    assert_eq!(bus.inner.peek(0x1F_0000), 0x22);
}

#[test]
fn specialized_vdc_tick_split_matches_division() {
    for pixel_clock in [
        VcePixelClock::DivideByFour,
        VcePixelClock::DivideByThree,
        VcePixelClock::DivideByTwo,
    ] {
        let divisor = u64::from(pixel_clock.divisor());
        for total in 0..=PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE + 3 {
            assert_eq!(
                split_vdc_master_ticks(total, pixel_clock),
                (total / divisor, (total % divisor) as u8)
            );
        }
    }
}
