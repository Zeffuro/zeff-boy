use super::*;

fn serialized() -> Vec<u8> {
    let mut bytes = vec![0; crate::serialization::retro_serialize_size()];
    assert!(crate::serialization::retro_serialize(
        bytes.as_mut_ptr().cast(),
        bytes.len()
    ));
    bytes
}

#[test]
fn md_advertised_capacity_accepts_maximum_queue_and_rejects_undersized_buffer() {
    let _session = Session::new();
    assert!(load(&md::guest(), "capacity.md"));
    {
        let mut core = lock(&CORE);
        let state = core.as_mut().unwrap();
        for _ in 0..6 {
            state.step_frame();
            assert_eq!(state.take_runtime_fault(), None);
        }
    }
    let capacity = crate::serialization::retro_serialize_size();
    assert_eq!(capacity, zeff_md_core::MAX_FRONTEND_STATE_BYTES + 4100);
    let before = lock(&CORE).as_ref().unwrap().encode_state().unwrap();
    let mut short = vec![0xa5; capacity - 1];
    assert!(!crate::serialization::retro_serialize(
        short.as_mut_ptr().cast(),
        short.len()
    ));
    assert!(short.iter().all(|value| *value == 0xa5));
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        before
    );
    let bytes = serialized();
    let payload_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert!(payload_len <= zeff_md_core::MAX_FRONTEND_STATE_BYTES);
    let payload = &bytes[4..4 + payload_len];
    assert_eq!(payload, before);
    let core_len = u32::from_le_bytes(payload[12..16].try_into().unwrap()) as usize;
    let audio_at = 20 + core_len;
    assert_eq!(payload.len() - audio_at, 16_988);
    assert_eq!(
        u32::from_le_bytes(payload[audio_at + 600..audio_at + 604].try_into().unwrap()),
        4096
    );
    assert!(crate::serialization::retro_unserialize(
        bytes.as_ptr().cast(),
        bytes.len()
    ));
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        before
    );
}

#[test]
fn rejected_md_payload_retains_existing_dirty_sram_import_boundary() {
    let _session = Session::new();
    assert!(load(&md::guest(), "dirty-save.md"));
    crate::retro_run();
    let mut bytes = serialized();
    let core_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let audio_at = 24 + core_len;
    bytes[audio_at + 599] = 34;
    let save = crate::memory::retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).cast::<u8>();
    assert!(!save.is_null());
    let mut expected = {
        let core = lock(&CORE);
        let state = core.as_ref().unwrap();
        let mut emu = machine(state).clone();
        let mut dirty = state.battery_sram().unwrap();
        dirty[3] = 0x44;
        emu.load_save_ram(&dirty).unwrap();
        emu
    };
    unsafe { save.add(3).write(0x44) };
    // SRAM import precedes payload validation in the existing whole-ABI contract.
    assert!(!crate::serialization::retro_unserialize(
        bytes.as_ptr().cast(),
        bytes.len()
    ));
    assert_eq!(unsafe { save.add(3).read() }, 0x44);
    let mut core = lock(&CORE);
    let state = core.as_mut().unwrap();
    assert_eq!(state.battery_sram().unwrap()[3], 0x44);
    assert_eq!(
        state.encode_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    state.step_frame();
    expected.step_field().unwrap();
    state.drain_audio();
    assert_eq!(state.audio_buf, expected.drain_audio_samples());
    assert_eq!(
        machine(state).encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
}
