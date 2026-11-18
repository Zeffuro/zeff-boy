use super::*;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn fixture(region: Region, r12: u8, r1: u8, at: u64) -> (VdpInterrupts, VdpRegisterPipeline) {
    let mut image = [0; 24];
    image[1] = r1;
    image[12] = r12;
    let mut registers = VdpRegisterPipeline::clock_fixture(tick(0), tick(0), image);
    let mut irq = VdpInterrupts::at_counters_zero(tick(0), 0, false, false);
    advance(&mut irq, &mut registers, region, at);
    (irq, registers)
}

fn advance(irq: &mut VdpInterrupts, registers: &mut VdpRegisterPipeline, region: Region, at: u64) {
    while irq
        .advance_pair(tick(at), region, registers, false)
        .unwrap()
        .is_some()
    {}
}

#[test]
fn production_publication_anchors_once_and_latches_at_hc0() {
    for (region, r1) in [
        (Region::NtscUs, 4),
        (Region::NtscJapan, 4),
        (Region::Pal, 4),
        (Region::Pal, 12),
    ] {
        for old in [0, 0x81] {
            for (queued, publication, boundary) in
                [(100, 120, 3420), (3401, 3420, 3420), (3421, 3440, 6840)]
            {
                let (mut irq, mut registers) = fixture(region, old, r1, queued);
                assert_eq!(
                    registers.queue_control_register(12, 1).unwrap().publish_at,
                    tick(publication)
                );
                assert!(!irq.has_timing_history());
                advance(&mut irq, &mut registers, region, publication - 1);
                assert!(!irq.has_timing_history());
                assert_eq!(
                    irq.next_row_deadline(region, &registers).unwrap(),
                    tick(boundary)
                );
                let row = irq
                    .completed_row(tick(boundary), region, &registers)
                    .unwrap()
                    .unwrap();
                assert_eq!(row.1.line_ticks, 3420);
                advance(&mut irq, &mut registers, region, publication);
                assert!(irq.has_timing_history());
                advance(&mut irq, &mut registers, region, boundary);
                assert_eq!(irq.effective_line_ticks(), 3360);
                assert_eq!(
                    irq.field_position(region, &registers).unwrap().line_start,
                    tick(boundary)
                );
                assert!(
                    irq.is_machine_write_slot(tick(boundary), region, &registers)
                        .unwrap()
                );
                assert_eq!(
                    irq.next_write_slot(tick(boundary), region, &registers)
                        .unwrap(),
                    tick(boundary + 16)
                );
                assert!(!irq.progressive_skip_allowed(&registers));
                assert!(irq.interval_end(region, &registers).is_none());
                for returned in [0, 0x81] {
                    let (mut returned_irq, mut returned_registers) = (irq, registers);
                    advance(
                        &mut returned_irq,
                        &mut returned_registers,
                        region,
                        boundary + 1,
                    );
                    returned_registers
                        .queue_control_register(12, returned)
                        .unwrap();
                    advance(
                        &mut returned_irq,
                        &mut returned_registers,
                        region,
                        boundary + 3360,
                    );
                    assert!(returned_irq.has_timing_history());
                    assert_eq!(returned_irq.effective_line_ticks(), 3420);
                    assert_eq!(
                        returned_irq
                            .field_position(region, &returned_registers)
                            .unwrap()
                            .line_start,
                        tick(boundary + 3360)
                    );
                }
            }
        }
    }
}

#[test]
fn production_sound_snapshot_keeps_seeded_and_new_pulse_deadlines() {
    for old in [0, 0x81] {
        let onset = 224 * 3420 + if old == 0 { 10 } else { 8 };
        let (mut irq, mut registers) = fixture(Region::NtscUs, old, 4, onset + 100);
        registers.queue_control_register(12, 1).unwrap();
        let snapshot = irq.sound_interrupt(&registers, Region::NtscUs);
        assert!(snapshot.asserted_at(onset + 3419).unwrap());
        assert!(!snapshot.asserted_at(onset + 3420).unwrap());
        advance(&mut irq, &mut registers, Region::NtscUs, onset + 120);
        let anchored = irq.sound_interrupt(&registers, Region::NtscUs);
        assert!(anchored.asserted_at(onset + 3419).unwrap());
        assert!(!anchored.asserted_at(onset + 3420).unwrap());
        assert!(anchored.asserted_at(onset).is_err());
        let end = irq
            .field_position(Region::NtscUs, &registers)
            .unwrap()
            .end()
            .unwrap()
            .get();
        advance(
            &mut irq,
            &mut registers,
            Region::NtscUs,
            end + 224 * 3360 + 7,
        );
        let snapshot = irq.sound_interrupt(&registers, Region::NtscUs);
        let onset = end + 224 * 3360 + 8;
        assert!(!snapshot.asserted_at(onset - 1).unwrap());
        assert!(snapshot.asserted_at(onset).unwrap());
        assert!(!snapshot.asserted_at(onset + 3360).unwrap());
    }
}

#[test]
fn production_chunking_and_projected_field_wrap_agree() {
    let region = Region::Pal;
    let (mut whole, mut whole_registers) = fixture(region, 0x81, 12, 100);
    whole_registers.queue_control_register(12, 1).unwrap();
    let (mut chunked, mut chunked_registers) = (whole, whole_registers);
    let end = 3420 + (region.frame_lines() - 1) * 3360;
    assert_eq!(
        whole
            .field_position(region, &whole_registers)
            .unwrap()
            .end()
            .unwrap(),
        tick(end)
    );
    advance(&mut whole, &mut whole_registers, region, end + 7777);
    let mut now = 100;
    while now < end + 7777 {
        now = (now + 2719).min(end + 7777);
        advance(&mut chunked, &mut chunked_registers, region, now);
    }
    assert_eq!(whole, chunked);
    assert_eq!(whole_registers, chunked_registers);
    assert_eq!(whole.field_count(), 1);
    assert_eq!(
        whole
            .field_position(region, &whole_registers)
            .unwrap()
            .start,
        tick(end)
    );
}
