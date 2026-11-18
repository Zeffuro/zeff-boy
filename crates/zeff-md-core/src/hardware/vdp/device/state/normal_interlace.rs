use super::*;

fn rejected(original: &Vdp, bytes: &[u8], region: Region) {
    let mut target = original.clone();
    assert!(
        target
            .read_state(&mut StateReader::new(bytes), region)
            .is_err()
    );
    assert_eq!(&target, original);
}

#[test]
fn normal_interlace_pending_published_and_latched_states_roundtrip_atomically() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, 0x81] {
            let mut vdp = transfer_fixture(region, horizontal, false);
            vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
            let pending = vdp.queue_register(12, horizontal | 2, region).unwrap();
            assert_eq!(restored(&vdp, region), vdp);

            vdp.advance_to(pending.publish_at, region).unwrap();
            assert_eq!(restored(&vdp, region), vdp);

            vdp.advance_to(tick(300 * H32_LINE_TICKS), region).unwrap();
            let saved = encoded(&vdp, region);
            assert_eq!(restored(&vdp, region), vdp);

            let mut reader = StateReader::new(&saved);
            VdpRegisterPipeline::read_state(&mut reader).unwrap();
            let calendar = reader.position();
            for offset in [32, 34] {
                for tag in [3, 127, 255] {
                    let mut malformed = saved.clone();
                    malformed[calendar + offset] = tag;
                    rejected(&vdp, &malformed, region);
                }
            }
        }
    }
}

#[test]
fn normal_interlace_deferred_write_preserves_partial_fifo_and_read_phase() {
    for region in [Region::NtscUs, Region::Pal] {
        for horizontal in [0, 0x81] {
            for code in [VRAM_WRITE_CODE, VRAM_READ_CODE] {
                let mut vdp = transfer_fixture(region, horizontal, true);
                command(&mut vdp, 0, code, region);
                if code == VRAM_WRITE_CODE {
                    vdp.write_data(0xcafe, region).unwrap();
                }
                let at = next_slot(&vdp, region);
                vdp.advance_to(at, region).unwrap();
                vdp.advance_to(tick(at.get() + 1), region).unwrap();
                let original = vdp.clone();
                let write = vdp.queue_register(12, horizontal | 2, region).unwrap();
                assert_eq!(vdp.state.fifo, original.state.fifo);
                assert_eq!(vdp.state.cache, original.state.cache);
                assert_eq!(vdp.state.read_phase, original.state.read_phase);
                let mut copy = restored(&vdp, region);
                for at in [write.publish_at.get(), 300 * H32_LINE_TICKS] {
                    vdp.advance_to(tick(at), region).unwrap();
                    copy.advance_to(tick(at), region).unwrap();
                    assert_eq!(vdp, copy);
                }
            }
        }
    }
}
