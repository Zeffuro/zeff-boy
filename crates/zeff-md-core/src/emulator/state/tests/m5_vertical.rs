use super::*;

fn advance(source: &mut Emulator, at: u64) {
    source
        .bus
        .advance_machine(at - source.bus.now.get())
        .unwrap();
}

fn control(source: &mut Emulator, word: u16) {
    let region = source.region();
    let begin = source.bus.now.get() + 40;
    advance(source, begin);
    source
        .bus
        .vdp
        .begin_timed_control(
            MasterTicks::new(begin),
            MasterTicks::new(begin + 28),
            region,
        )
        .unwrap();
    advance(source, begin + 28);
    source.bus.vdp.write_control(word, region).unwrap();
    source
        .bus
        .vdp
        .finish_timed_control(MasterTicks::new(begin + 28), region)
        .unwrap();
    if let Some(write) = source.bus.vdp.registers().pending() {
        advance(source, write.publish_at.get());
    }
}

fn future(source: &Emulator) {
    let raw = source.encode_state().unwrap();
    let frontend = source.encode_frontend_state().unwrap();
    let mut whole = source.clone();
    let mut restored = source.clone();
    restored.decode_frontend_state(&frontend).unwrap();
    let mut core = source.clone();
    core.decode_state(&raw).unwrap();
    assert_eq!(restored.encode_frontend_state().unwrap(), frontend);
    assert_eq!(core.encode_state().unwrap(), raw);
    for _ in 0..16 {
        let result = whole.step_instruction();
        assert_eq!(restored.step_instruction(), result);
        assert_eq!(core.step_instruction(), result);
        result.unwrap();
        let expected = whole.encode_state().unwrap();
        assert_eq!(restored.encode_state().unwrap(), expected);
        assert_eq!(core.encode_state().unwrap(), expected);
        assert_eq!(restored.framebuffer(), whole.framebuffer());
    }
}

fn key(raw: &[u8]) -> usize {
    let (_, video_at) = layout::section_offsets(raw);
    let mut reader = StateReader::new(raw);
    reader.set_position(video_at + 15 + crate::constants::FRAMEBUFFER_LEN);
    assert!(reader.read_bool().unwrap());
    reader.set_position(reader.position() + 12);
    reader.read_u8().unwrap();
    if reader.read_bool().unwrap() {
        reader.read_u16().unwrap();
        reader.read_u16().unwrap();
    }
    assert!(reader.read_bool().unwrap());
    reader.position()
}

fn rejected(source: &Emulator, mut raw: Vec<u8>, field: u64, row: u16, error: &str) {
    let at = key(&raw);
    raw[at..at + 8].copy_from_slice(&field.to_le_bytes());
    raw[at + 8..at + 10].copy_from_slice(&row.to_le_bytes());
    let mut target = source.clone();
    assert!(
        target
            .decode_state(&raw)
            .unwrap_err()
            .to_string()
            .contains(error)
    );
    assert_eq!(&target, source);
    let frontend = source.encode_frontend_state().unwrap();
    let mut reader = StateReader::new(&frontend);
    reader.set_position(12);
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    let suffix = reader.position();
    let mut writer = StateWriter::new();
    writer.write_bytes(&frontend[..12]);
    writer.write_vec(&raw);
    writer.write_bytes(&frontend[suffix..]);
    assert!(
        target
            .decode_frontend_state(&writer.into_bytes())
            .unwrap_err()
            .to_string()
            .contains(error)
    );
    assert_eq!(&target, source);
}

pub(super) fn checks() {
    for region in [Region::NtscUs, Region::NtscJapan] {
        let mut source = machine(region, false);
        control(&mut source, 0x813c);
        advance(&mut source, 223 * crate::constants::VDP_LINE_TICKS + 100);
        control(&mut source, 0x8174);
        let final_line = 523 * crate::constants::VDP_LINE_TICKS;
        advance(&mut source, final_line + 3189);
        let before = source.encode_state().unwrap();
        let id = source.pacing_field_count();
        assert_eq!(id, 1);
        rejected(&source, before, id + 1, 0, "precedes its fetch");
        advance(&mut source, final_line + 3195);
        assert_eq!(source.bus.video.horizontal_scroll(id + 1, 0), Some([0, 0]));
        future(&source);
        let raw = source.encode_state().unwrap();
        rejected(&source, raw.clone(), id + 2, 0, "ahead of its calendar");
        rejected(&source, raw, id + 1, 2, "ahead of its calendar");

        let mut source = machine(region, false);
        control(&mut source, 0x817c);
        advance(&mut source, (512 + 241) * crate::constants::VDP_LINE_TICKS);
        assert_eq!(source.framebuffer_dimensions().1, 240);
        future(&source);
    }
}
