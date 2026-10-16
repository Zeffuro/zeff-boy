mod vdp_active_calendar {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn literal_grants_and_counter_coordinates() {
        cases::literal_grants_and_counter_coordinates();
    }

    #[test]
    fn every_master_tick_matches_literal_calendar() {
        cases::every_master_tick_matches_literal_calendar();
    }

    #[test]
    fn strict_edges_and_busy_slots() {
        cases::strict_edges_and_busy_slots();
    }

    #[test]
    fn epoch_and_overflow_are_pure() {
        cases::epoch_and_overflow_are_pure();
    }

    #[test]
    fn already_eligible_fifo_preserves_byte_phase() {
        cases::already_eligible_fifo_preserves_byte_phase();
    }
}

mod vdp_field_calendar {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn field_slots_and_counts() {
        cases::field_slots_and_counts();
    }

    #[test]
    fn every_field_phase() {
        cases::every_field_phase();
    }

    #[test]
    fn interval_endpoints() {
        cases::interval_endpoints();
    }

    #[test]
    fn partitioned_intervals() {
        cases::partitioned_intervals();
    }

    #[test]
    fn mode_and_tail_guards() {
        cases::mode_and_tail_guards();
    }

    #[test]
    fn fifo_service_and_uncertainty() {
        cases::fifo_service_and_uncertainty();
    }
}

mod vdp_h32_timing {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn counter_units_jump_and_wrap() {
        cases::counter_units_jump_and_wrap();
    }
    #[test]
    fn nonactive_slots_skip_refresh() {
        cases::nonactive_slots_skip_refresh();
    }
    #[test]
    fn calendar_drives_captured_fifo() {
        cases::calendar_drives_captured_fifo();
    }
    #[test]
    fn epoch_and_overflow_guards() {
        cases::epoch_and_overflow_guards();
    }
}

mod vdp_h40_timing {
    use zeff_emu_common::time::MasterTicks;
    use zeff_md_core::{Region, Vdp, VdpInterruptClock, VdpRegisterError, VdpVerticalTiming};

    const REGION: Region = Region::NtscUs;
    fn tick(value: u64) -> MasterTicks {
        MasterTicks::new(value)
    }

    fn register(device: &mut Vdp, index: u8, value: u8) {
        device
            .advance_to(tick(device.now().get() + 1), REGION)
            .unwrap();
        let write = device.queue_register(index, value, REGION).unwrap();
        device.advance_to(write.publish_at, REGION).unwrap();
    }

    fn device() -> Vdp {
        let mut device = Vdp::bootstrap_mode5();
        register(&mut device, 12, 0x81);
        device
    }

    pub fn counter_and_blank_edges() {
        for (phase, expected) in [
            (0, 0x0100),
            (8, 0x0100),
            (16, 0x0101),
            (2639, 0x01a4),
            (2640, 0x02a5),
            (2919, 0x02b6),
            (2920, 0x02e4),
            (2952, 0x02e6),
            (2961, 0x02e6),
            (2962, 0x02e7),
            (3266, 0x02f6),
            (3275, 0x02f6),
            (3276, 0x02f7),
            (3419, 0x02ff),
            (3420, 0x0200),
        ] {
            let mut device = device();
            device.advance_to(tick(3420 + phase), REGION).unwrap();
            assert_eq!(device.read_hv(REGION), Ok(expected));
        }
        for (phase, blank) in [(91, true), (92, false), (2867, false), (2868, true)] {
            let mut device = device();
            device.advance_to(tick(phase), REGION).unwrap();
            assert_eq!(device.read_status(REGION).unwrap() & 4 != 0, blank);
        }
    }

    pub fn vertical_and_interrupt_coordinates() {
        let mut device = device();
        register(&mut device, 1, 0x44);
        let timing = VdpVerticalTiming::at_counters_zero(tick(0));
        let registers = device.registers();
        assert_eq!(timing.v_counter_at(tick(2639), REGION, registers), Ok(0));
        assert_eq!(timing.v_counter_at(tick(2640), REGION, registers), Ok(1));
        let horizontal = timing
            .next_interrupt_edge(tick(2639), REGION, registers)
            .unwrap();
        assert_eq!(
            (horizontal.at, horizontal.clock),
            (tick(2640), VdpInterruptClock::HorizontalCounter)
        );
        let vertical = timing
            .next_interrupt_edge(tick(224 * 3420 + 7), REGION, registers)
            .unwrap();
        assert_eq!(
            (vertical.at, vertical.clock),
            (tick(224 * 3420 + 8), VdpInterruptClock::Vertical)
        );
        for (at, expected) in [
            (223 * 3420 + 2639, false),
            (223 * 3420 + 2640, true),
            (260 * 3420 + 2639, true),
            (260 * 3420 + 2640, false),
        ] {
            assert_eq!(
                timing.vblank_flag_at(tick(at), REGION, registers),
                Ok(expected)
            );
        }
        assert_eq!(
            timing.v_counter_at(tick(234 * 3420 + 2640), REGION, registers),
            Ok(0x1e5)
        );
        assert_eq!(
            timing.v_counter_at(tick(261 * 3420 + 2640), REGION, registers),
            Ok(0)
        );
    }

    pub fn slot_calendar() {
        let mut device = device();
        let timing = VdpVerticalTiming::at_counters_zero(tick(0));
        let mut at = tick(3419);
        let mut slots = Vec::new();
        while let Ok(next) = timing.next_memory_slot(at, REGION, device.registers()) {
            if next.get() >= 6840 {
                break;
            }
            assert_eq!(
                timing.is_memory_slot(next, REGION, device.registers()),
                Ok(true)
            );
            slots.push(next.get() - 3420);
            at = next;
        }
        assert_eq!(slots.len(), 204);
        assert_eq!(&slots[..4], &[0, 16, 32, 48]);
        assert!(!slots.contains(&400));
        assert!(slots.contains(&2962));
        assert!(!slots.contains(&2960));
        register(&mut device, 1, 0x44);
        let expected = [
            16, 144, 272, 528, 656, 784, 1040, 1168, 1296, 1552, 1680, 1808, 2064, 2192, 2320,
            2560, 2576, 2962,
        ];
        let mut after = tick(0);
        for offset in expected {
            let next = timing
                .next_memory_slot(after, REGION, device.registers())
                .unwrap();
            assert_eq!(next, tick(offset));
            after = next;
        }
        assert_eq!(
            timing.next_memory_slot(after, REGION, device.registers()),
            Ok(tick(3436))
        );
    }

    pub fn mixed_mode_atomicity() {
        let mut device = device();
        for value in [1, 0x80, 0x83, 0x85] {
            let saved = device.clone();
            assert_eq!(
                device.queue_register(12, value, REGION),
                Err(VdpRegisterError::UnsupportedModeChange { index: 12, value }.into())
            );
            assert_eq!(device, saved);
        }
        register(&mut device, 12, 0);
        device.advance_to(tick(2960), REGION).unwrap();
        assert_eq!(device.read_hv(REGION), Ok(0x01e9));
    }

    #[test]
    fn counters() {
        counter_and_blank_edges();
    }
    #[test]
    fn interrupts() {
        vertical_and_interrupt_coordinates();
    }
    #[test]
    fn slots() {
        slot_calendar();
    }
    #[test]
    fn guards() {
        mixed_mode_atomicity();
    }
}

mod vdp_hblank {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn full_lines() {
        cases::full_lines();
    }
    #[test]
    fn literal_edges_and_copies() {
        cases::literal_edges_and_copies();
    }
    #[test]
    fn epoch_and_overflow_guards() {
        cases::epoch_and_overflow_guards();
    }
    #[test]
    fn flag_edges_do_not_service_fifo() {
        cases::flag_edges_do_not_service_fifo();
    }
}

mod vdp_vertical {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn counter_sequence() {
        cases::counter_sequence();
    }
    #[test]
    fn literal_edges_and_copies() {
        cases::literal_edges_and_copies();
    }
    #[test]
    fn critical_lines() {
        cases::critical_lines();
    }
    #[test]
    fn epoch_and_overflow_guards() {
        cases::epoch_and_overflow_guards();
    }
    #[test]
    fn selector_partition() {
        cases::selector_partition();
    }
    #[test]
    fn pending_publication() {
        cases::pending_publication();
    }
}

mod pal_timing {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn vertical_counter_and_blank_edges() {
        cases::vertical_counter_and_blank_edges();
    }

    #[test]
    fn interrupt_calendar_and_hint_counter() {
        cases::interrupt_calendar_and_hint_counter();
    }

    #[test]
    fn acknowledge_edges_and_status() {
        cases::acknowledge_edges_and_status();
    }

    #[test]
    fn memory_admission_and_horizontal_slots() {
        cases::memory_admission_and_horizontal_slots();
    }

    #[test]
    fn long_skip_and_ntsc_regression() {
        cases::long_skip_and_ntsc_regression();
    }

    #[test]
    fn unsupported_modes_and_overflow() {
        cases::unsupported_modes_and_overflow();
    }

    #[test]
    fn v30_counter_interrupt_and_memory_edges() {
        cases::v30_counter_interrupt_and_memory_edges();
    }
}

mod vdp_interrupts {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn event_calendar() {
        cases::event_calendar();
    }
    #[test]
    fn line_counter_partition() {
        cases::line_counter_partition();
    }
    #[test]
    fn line_counter_frames() {
        cases::line_counter_frames();
    }
    #[test]
    fn pending_enable_acknowledge() {
        cases::pending_enable_acknowledge();
    }
    #[test]
    fn reload_publication() {
        cases::reload_publication();
    }
    #[test]
    fn fault_atomicity() {
        cases::fault_atomicity();
    }
    #[test]
    fn event_order_and_copy() {
        cases::event_order_and_copy();
    }
}

mod vdp_order {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn ordered_publications() {
        cases::ordered_publications();
    }
    #[test]
    fn phase_and_fault_guards() {
        cases::phase_and_fault_guards();
    }
    #[test]
    fn collision_guards() {
        cases::collision_guards();
    }
    #[test]
    fn acknowledge_ordering() {
        cases::acknowledge_ordering();
    }
    #[test]
    fn chunk_and_copy() {
        cases::chunk_and_copy();
    }
    #[test]
    fn actual_cpu_acknowledge() {
        cases::actual_cpu_acknowledge();
    }
    #[test]
    fn actual_cpu_publications() {
        cases::actual_cpu_publications();
    }
    #[test]
    fn actual_cpu_faults() {
        cases::actual_cpu_faults();
    }
}

mod vdp_register_order {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn disjoint_table_and_counter_publications() {
        cases::disjoint_table_and_counter_publications();
    }
    #[test]
    fn old_reload_precedes_new_register() {
        cases::old_reload_precedes_new_register();
    }
    #[test]
    fn mask_publication_routes_existing_latch() {
        cases::mask_publication_routes_existing_latch();
    }

    #[test]
    fn hv_latch_at_tied_publication() {
        cases::hv_latch_at_tied_publication();
    }
}

mod vdp_device {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn bootstrap_and_display_off() {
        cases::bootstrap_and_display_off();
    }
    #[test]
    fn arbitrary_counter_and_settled_cycles() {
        cases::arbitrary_counter_and_settled_cycles();
    }
    #[test]
    fn pending_reload_and_chunking() {
        cases::pending_reload_and_chunking();
    }
    #[test]
    fn mode_and_clock_atomicity() {
        cases::mode_and_clock_atomicity();
    }
    #[test]
    fn acknowledge_endpoints() {
        cases::acknowledge_endpoints();
    }
    #[test]
    fn maximum_clock_jump() {
        cases::maximum_clock_jump();
    }
}

mod vdp_capture {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn upper_address_command_bits() {
        cases::upper_address_command_bits();
    }
    #[test]
    fn address_increment_partition() {
        cases::address_increment_partition();
    }
    #[test]
    fn rejected_acceptance_preserves_state() {
        cases::rejected_acceptance_preserves_state();
    }
    #[test]
    fn published_increment_controls_fifo_capture() {
        cases::published_increment_controls_fifo_capture();
    }
    #[test]
    fn wide_entries_and_memory_guards() {
        cases::wide_entries_and_memory_guards();
    }
}

mod vdp_line_capture {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn publication_deadlines_and_replay() {
        cases::publication_deadlines_and_replay();
    }

    #[test]
    fn fifo_completion_is_inclusive() {
        cases::fifo_completion_is_inclusive();
    }

    #[test]
    fn aliased_vram_rollback() {
        cases::aliased_vram_rollback();
    }

    #[test]
    fn mixed_memory_and_sprite_rollback() {
        cases::mixed_memory_and_sprite_rollback();
    }

    #[test]
    fn read_completion_is_inclusive() {
        cases::read_completion_is_inclusive();
    }

    #[test]
    fn schedule_and_render_guards() {
        cases::schedule_and_render_guards();
    }
}
