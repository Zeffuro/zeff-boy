use super::*;

fn mode(machine: &mut Emulator, value: u8) {
    let region = machine.region();
    machine
        .bus
        .vdp
        .write_control(0x8100 | u16::from(value), region)
        .unwrap();
    advance(machine, 20);
}

fn pair(left: &mut Emulator, right: &mut Emulator) {
    assert!(left == right);
    assert_eq!(left.encode_state().unwrap(), right.encode_state().unwrap());
    assert_eq!(
        left.encode_frontend_state().unwrap(),
        right.encode_frontend_state().unwrap()
    );
    assert_eq!(left.framebuffer_available(), right.framebuffer_available());
    assert_eq!(left.frame_ready(), right.frame_ready());
    assert_eq!(left.drain_audio_samples(), right.drain_audio_samples());
}

pub(super) fn availability_after_render_failure() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut source = machine(region);
        advance(&mut source, region.frame_ticks());
        assert!(source.framebuffer_available() && source.frame_ready());
        let field = source.bus.vdp.field_position(region).unwrap();
        let old_mode = source.vdp().registers().registers()[1].unwrap();
        let old_frames = source.frame_count();
        mode(&mut source, old_mode | 0x80);
        advance(&mut source, field.line_ticks);
        assert!(
            matches!(source.video_error(), Some(crate::VdpLineError::Render(crate::VdpRenderError::UnsupportedRenderMode(value))) if value == (old_mode | 0x80))
        );
        assert!(!source.framebuffer_available());
        assert!(!source.frame_ready());
        assert_eq!(source.frame_count(), old_frames);
        let before = source.clone();
        assert!(source.encode_state().is_err());
        assert!(source.encode_frontend_state().is_err());
        assert!(source == before);
        mode(&mut source, old_mode);
        let end = field.end().unwrap().get();
        let ticks = end - 1 - source.bus.now.get();
        advance(&mut source, ticks);
        assert!(!source.framebuffer_available() && !source.frame_ready());
        assert_eq!(source.frame_count(), old_frames);
        assert!(source.video_error().is_some());
        advance(&mut source, 2);
        advance(&mut source, field.line_ticks);
        assert!(source.video_error().is_none());
        assert!(!source.framebuffer_available() && !source.frame_ready());
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        let mut raw_reference = source.clone();
        raw_reference.decode_state(&raw).unwrap();
        let mut raw_consumer = machine(region);
        raw_consumer.decode_state(&raw).unwrap();
        let mut frontend_consumer = machine(region);
        frontend_consumer.decode_frontend_state(&frontend).unwrap();
        pair(&mut source, &mut frontend_consumer);
        pair(&mut raw_reference, &mut raw_consumer);
        let ticks = (source.framebuffer_dimensions().1 as u64 - 1) * field.line_ticks;
        for machine in [
            &mut source,
            &mut frontend_consumer,
            &mut raw_reference,
            &mut raw_consumer,
        ] {
            advance(machine, ticks);
            assert!(machine.framebuffer_available() && machine.frame_ready());
            assert_eq!(machine.video_error(), None);
            assert_eq!(machine.frame_count(), field.id + 2);
        }
        pair(&mut source, &mut frontend_consumer);
        pair(&mut raw_reference, &mut raw_consumer);
    }
}
