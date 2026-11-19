use super::*;

fn cold(region: Region) -> Emulator {
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    bytes[0x100..0x104].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound: region == Region::Pal,
        },
    )
    .unwrap()
}

fn overwrite(machine: &mut Emulator) {
    let region = machine.region();
    command(machine, 0, 3);
    machine.bus.vdp.write_data(0x0246, region).unwrap();
    advance(machine, 1000);
    assert_eq!(machine.vdp().memory().cram_word(0), Some(0x0246));
}

pub(super) fn cold_fill_presentations_restore() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = cold(region);
        for (index, value) in [(1, 0x14), (15, 2), (19, 1), (23, 0x80)] {
            source
                .bus
                .vdp
                .write_control(0x8000 | (index << 8) | value, region)
                .unwrap();
            advance(&mut source, 20);
        }
        command(&mut source, 126, 0x23);
        source.bus.vdp.write_data(0xabcd, region).unwrap();
        advance(&mut source, 1000);
        assert_eq!(source.vdp().memory().cram_known_mask(), u64::MAX ^ 1);
        assert!(source.framebuffer_available());
        #[cfg(feature = "first-failure-diagnostic")]
        {
            use crate::diagnostic::field::{Capture, Kind, cause};
            let mut observed = source.clone();
            let capture = Capture::begin().unwrap();
            capture.after_load(&observed);
            observed.step_field().unwrap();
            observed.step_field().unwrap();
            let report = capture.finish(&observed).unwrap();
            assert_eq!(report.issues, 0);
            assert!(report.events.iter().flatten().any(|event| matches!(
                event.kind,
                Kind::RowTransition { causes, .. } if causes & cause::RENDER_ERROR != 0
            )));
            assert!(!observed.framebuffer_available());
        }

        let mut early = source.clone();
        overwrite(&mut early);
        early.step_field().unwrap();
        assert!(early.framebuffer_available() && early.frame_ready());
        let current_owner = early.encode_frontend_state().unwrap();
        early.decode_frontend_state(&current_owner).unwrap();
        let legacy = early.encode_state().unwrap();
        let mut imported = cold(region);
        imported.decode_state(&legacy).unwrap();
        assert_eq!(imported.vdp().memory().cram_known_mask(), u64::MAX);
        assert!(imported.framebuffer_available());
        assert_eq!(
            imported.encode_state().unwrap(),
            early.encode_state().unwrap()
        );

        advance(&mut source, 4000);
        assert!(!source.framebuffer_available());
        assert!(!source.frame_ready());
        assert!(source.video_error().is_none());
        assert_eq!(
            source.framebuffer().len(),
            source.framebuffer_dimensions().0 * source.framebuffer_dimensions().1 * 4
        );
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        let mut from_raw = cold(region);
        from_raw.decode_state(&raw).unwrap();
        let mut raw_reference = source.clone();
        raw_reference.decode_state(&raw).unwrap();
        let mut from_frontend = cold(region);
        from_frontend.decode_frontend_state(&frontend).unwrap();
        assert_eq!(from_raw.encode_state().unwrap(), raw);
        assert_eq!(from_frontend.encode_frontend_state().unwrap(), frontend);
        for machine in [
            &mut source,
            &mut from_raw,
            &mut raw_reference,
            &mut from_frontend,
        ] {
            assert!(!machine.framebuffer_available());
            overwrite(machine);
            assert_eq!(machine.vdp().memory().cram_known_mask(), u64::MAX);
            assert!(!machine.framebuffer_available());
        }
        let midway = source.encode_frontend_state().unwrap();
        from_frontend.decode_frontend_state(&midway).unwrap();
        assert!(!from_frontend.framebuffer_available());
        for field in 0..2 {
            for machine in [
                &mut source,
                &mut from_raw,
                &mut raw_reference,
                &mut from_frontend,
            ] {
                machine.step_field().unwrap();
                assert!(machine.video_error().is_none());
                assert_eq!(machine.framebuffer_available(), field == 1);
                assert_eq!(machine.frame_ready(), field == 1);
            }
            assert_eq!(
                source.encode_state().unwrap(),
                from_frontend.encode_state().unwrap()
            );
            assert_eq!(
                raw_reference.encode_state().unwrap(),
                from_raw.encode_state().unwrap()
            );
            assert_eq!(
                source.drain_audio_samples(),
                from_frontend.drain_audio_samples()
            );
            assert_eq!(
                raw_reference.drain_audio_samples(),
                from_raw.drain_audio_samples()
            );
        }
    }
}
