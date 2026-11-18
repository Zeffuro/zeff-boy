use super::*;

#[test]
fn normal_interlace_width_publications_keep_both_vc_edges_on_one_side() {
    for (region, v30) in CONFIGURATIONS {
        for (old, new) in [(0, 0x8b), (0x81, 0x0a), (2, 0x8b), (0x83, 0x0a)] {
            for phase in [2620, 2640, 2660, 2680] {
                let at = LINE + phase;
                let mut vdp = fixture(region, old, false, v30, 0, at - 1);
                let before = vdp.clone();
                let result = vdp
                    .write_control(clock_word(new), region)
                    .map(|_| vdp.registers().pending().unwrap());
                if phase == 2640 {
                    assert!(matches!(
                        result,
                        Err(VdpPortError::Schedule(VdpScheduleError::Register(
                            VdpRegisterError::UnsupportedModeChange { index: 12, .. }
                        )))
                    ));
                    assert_eq!(vdp, before);
                } else {
                    let write = result.unwrap();
                    assert_eq!(write.publish_at.get(), at);
                    let mut copy = restored(&vdp, region);
                    advance(
                        &mut vdp,
                        std::slice::from_mut(&mut copy),
                        at + H32_LINE_TICKS,
                        region,
                    );
                    assert_eq!(
                        vdp.read_hv(region).unwrap() >> 8,
                        if phase >= 2660 { 22 } else { 21 }
                    );
                }
            }
        }
    }
}

#[test]
fn normal_interlace_width_changes_reject_live_history_fifo_and_partial_read() {
    for (region, v30) in CONFIGURATIONS {
        for (old, new) in [(0x0a, 0x8b), (0x8b, 0x0a)] {
            let mut vdp = fixture(region, old, false, v30, 0, 330 * H32_LINE_TICKS + 101);
            let before = vdp.clone();
            assert!(vdp.queue_register(12, new, region).is_err());
            assert_eq!(vdp, before);
        }
        for (old, new) in [(0, 0x8b), (0x81, 0x0a)] {
            for code in [VRAM_WRITE_CODE, VRAM_READ_CODE] {
                let mut vdp = fixture(region, old, false, v30, 0, LINE + 101);
                command(&mut vdp, 0, code, region);
                if code == VRAM_WRITE_CODE {
                    vdp.write_data(0xcafe, region).unwrap();
                }
                let slot = vdp
                    .state
                    .interrupts
                    .next_write_slot(vdp.now(), region, &vdp.state.registers)
                    .unwrap();
                vdp.advance_to(slot, region).unwrap();
                vdp.advance_to(tick(slot.get() + 1), region).unwrap();
                assert!(if code == VRAM_WRITE_CODE {
                    !vdp.state.fifo.is_empty()
                } else {
                    vdp.state.read_phase.is_some()
                });
                let before = vdp.clone();
                assert!(vdp.queue_register(12, new, region).is_err());
                assert_eq!(vdp, before);
            }
        }
    }
}
