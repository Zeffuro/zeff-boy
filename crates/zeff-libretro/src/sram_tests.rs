use crate::api::{RETRO_MEMORY_SAVE_RAM, retro_game_info};
use crate::callbacks::{
    ABI_TEST_LOCK, CB_INPUT_POLL, CB_VIDEO_REFRESH, CORE, FRAME_COUNTER, MAX_SERIALIZE_SIZE, lock,
};
use std::ffi::CString;
use std::os::raw::{c_uint, c_void};

struct AbiSession {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl AbiSession {
    fn new() -> Self {
        let guard = lock(&ABI_TEST_LOCK);
        reset_globals();
        Self { _guard: guard }
    }
}

impl Drop for AbiSession {
    fn drop(&mut self) {
        reset_globals();
    }
}

fn reset_globals() {
    *lock(&CORE) = None;
    lock(&crate::sram::SRAM).clear();
    *lock(&MAX_SERIALIZE_SIZE) = 0;
    *lock(&FRAME_COUNTER) = 0;
    *lock(&CB_INPUT_POLL) = None;
    *lock(&CB_VIDEO_REFRESH) = None;
}

fn gba_sram_rom() -> Vec<u8> {
    gba_backup_rom(b"SRAM_V113")
}

fn gba_backup_rom(marker: &[u8]) -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(marker);
    rom
}

fn gba_program_writing_sram_rom() -> Vec<u8> {
    let mut rom = gba_sram_rom();
    rom[0..4].copy_from_slice(&0xE59F_0008_u32.to_le_bytes()); // ldr r0, [pc, #8]
    rom[4..8].copy_from_slice(&0xE3A0_105A_u32.to_le_bytes()); // mov r1, #0x5A
    rom[8..12].copy_from_slice(&0xE5C0_1000_u32.to_le_bytes()); // strb r1, [r0]
    rom[12..16].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes()); // b .
    rom[16..20].copy_from_slice(&0x0E00_0000_u32.to_le_bytes());
    rom
}

fn gb_sram_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x147] = 0x03;
    rom[0x149] = 0x02;
    rom
}

fn load_rom(rom: &[u8], path: &str) {
    let path = CString::new(path).unwrap();
    let info = retro_game_info {
        path: path.as_ptr(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        meta: std::ptr::null(),
    };
    assert!(crate::game::retro_load_game(&info));
}

fn save_memory() -> (*mut u8, usize) {
    let size = crate::memory::retro_get_memory_size(RETRO_MEMORY_SAVE_RAM);
    let data = crate::memory::retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).cast();
    (data, size)
}

fn write_save_byte(offset: usize, value: u8) {
    let (data, size) = save_memory();
    assert!(offset < size);
    unsafe { data.add(offset).write(value) };
}

fn exposed_save_byte(offset: usize) -> u8 {
    let (data, size) = save_memory();
    assert!(offset < size);
    unsafe { data.add(offset).read() }
}

fn exposed_save_bytes() -> Vec<u8> {
    let (data, size) = save_memory();
    if data.is_null() {
        assert_eq!(size, 0);
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data, size).to_vec() }
    }
}

fn core_save_byte(offset: usize) -> u8 {
    lock(&CORE)
        .as_ref()
        .and_then(|state| state.battery_sram())
        .unwrap()[offset]
}

fn replace_core_save_bytes(bytes: &[u8]) {
    lock(&CORE)
        .as_mut()
        .expect("loaded core")
        .load_battery_sram(bytes)
        .expect("synthetic backup data should load");
}

fn serialize() -> Vec<u8> {
    let size = crate::serialization::retro_serialize_size();
    let mut bytes = vec![0; size];
    assert!(crate::serialization::retro_serialize(
        bytes.as_mut_ptr().cast(),
        bytes.len()
    ));
    bytes
}

fn assert_stable_memory(expected: (*mut u8, usize)) {
    assert_eq!(save_memory(), expected);
}

unsafe extern "C" fn input_poll_writes_save_ram() {
    let data = crate::memory::retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).cast::<u8>();
    if !data.is_null() {
        unsafe { data.add(3).write(0xA3) };
    }
}

unsafe extern "C" fn video_refresh_writes_save_ram(
    _data: *const c_void,
    _width: c_uint,
    _height: c_uint,
    _pitch: usize,
) {
    let data = crate::memory::retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).cast::<u8>();
    if !data.is_null() {
        unsafe { data.add(3).write(0xB3) };
    }
}

#[test]
fn abi_save_ram_bridge_imports_external_writes_and_keeps_state_authoritative() {
    let _session = AbiSession::new();
    load_rom(&gba_sram_rom(), "bridge.gba");
    let stable = save_memory();
    assert!(!stable.0.is_null());
    assert_eq!(stable.1, 64 * 1024);
    assert_eq!(exposed_save_byte(0), core_save_byte(0));

    write_save_byte(0, 0x11);
    let startup = serialize();
    assert_eq!(core_save_byte(0), 0x11);
    assert_stable_memory(stable);

    write_save_byte(0, 0x22);
    crate::retro_run();
    assert_eq!(core_save_byte(0), 0x22);
    assert_eq!(exposed_save_byte(0), 0x22);
    assert_stable_memory(stable);

    write_save_byte(0, 0x33);
    assert!(crate::serialization::retro_unserialize(
        startup.as_ptr().cast(),
        startup.len()
    ));
    assert_eq!(core_save_byte(0), 0x11);
    assert_eq!(exposed_save_byte(0), 0x11);
    assert_stable_memory(stable);

    write_save_byte(0, 0x44);
    let invalid = vec![0; startup.len()];
    assert!(!crate::serialization::retro_unserialize(
        invalid.as_ptr().cast(),
        invalid.len()
    ));
    assert_eq!(core_save_byte(0), 0x44);
    assert_eq!(exposed_save_byte(0), 0x44);
    assert_stable_memory(stable);

    write_save_byte(0, 0x55);
    crate::retro_reset();
    assert_eq!(core_save_byte(0), 0x55);
    assert_eq!(exposed_save_byte(0), 0x55);
    assert_stable_memory(stable);

    write_save_byte(0, 0x66);
    assert!(!lock(&crate::sram::SRAM).is_published());
    crate::game::retro_unload_game();
    assert!(lock(&CORE).is_none());
    assert!(lock(&crate::sram::SRAM).is_published());
}

#[test]
fn abi_save_ram_bridge_repeatedly_publishes_exact_flash1m_and_eeprom_bytes_stably() {
    let _session = AbiSession::new();
    let cases: &[(&str, &[u8], usize)] = &[
        ("flash1m.gba", b"FLASH1M_V103", 128 * 1024),
        ("eeprom.gba", b"EEPROM_V122", 8 * 1024),
    ];

    for &(path, marker, expected_size) in cases {
        load_rom(&gba_backup_rom(marker), path);
        let stable = save_memory();
        assert!(!stable.0.is_null());
        assert_eq!(stable.1, expected_size);

        for generation in 0..3u8 {
            let expected: Vec<u8> = (0..expected_size)
                .map(|index| (index as u8).wrapping_mul(17).wrapping_add(generation))
                .collect();
            replace_core_save_bytes(&expected);

            crate::retro_run();

            assert_eq!(
                exposed_save_bytes(),
                expected,
                "{path}, publication {generation}"
            );
            assert_eq!(core_save_byte(0), expected[0]);
            assert_eq!(
                core_save_byte(expected_size - 1),
                expected[expected_size - 1]
            );
            assert!(lock(&crate::sram::SRAM).is_published());
            assert_stable_memory(stable);
        }

        crate::game::retro_unload_game();
    }
}

#[test]
fn abi_run_immediately_publishes_synthetic_core_sram_write() {
    let _session = AbiSession::new();
    load_rom(&gba_program_writing_sram_rom(), "core-write.gba");
    let stable = save_memory();
    assert_eq!(exposed_save_byte(0), 0xFF);

    crate::retro_run();

    assert_eq!(core_save_byte(0), 0x5A);
    assert_eq!(exposed_save_byte(0), 0x5A);
    assert!(lock(&crate::sram::SRAM).is_published());
    assert_stable_memory(stable);
}

#[test]
fn abi_save_ram_bridge_preserves_gb_rebuild_and_reports_no_save_memory() {
    let _session = AbiSession::new();
    load_rom(&gb_sram_rom(), "bridge.gb");
    let stable = save_memory();
    assert!(!stable.0.is_null());
    assert_eq!(stable.1, 8 * 1024);

    write_save_byte(7, 0xA7);
    crate::retro_reset();
    assert_eq!(core_save_byte(7), 0xA7);
    assert_eq!(exposed_save_byte(7), 0xA7);
    assert_stable_memory(stable);

    crate::game::retro_unload_game();
    load_rom(&vec![0; 0x8000], "no-save.gb");
    let (data, size) = save_memory();
    assert!(data.is_null());
    assert_eq!(size, 0);
}

#[test]
fn abi_run_imports_callback_writes_at_the_correct_frame_boundaries() {
    let _session = AbiSession::new();
    load_rom(&gba_sram_rom(), "callbacks.gba");
    let stable = save_memory();
    crate::retro_set_input_poll(input_poll_writes_save_ram);
    crate::retro_set_video_refresh(video_refresh_writes_save_ram);

    crate::retro_run();

    assert_eq!(core_save_byte(3), 0xA3);
    assert_eq!(exposed_save_byte(3), 0xB3);
    assert!(!lock(&crate::sram::SRAM).is_published());
    assert_stable_memory(stable);

    *lock(&CB_INPUT_POLL) = None;
    *lock(&CB_VIDEO_REFRESH) = None;
    serialize();
    assert_eq!(core_save_byte(3), 0xB3);
    assert_eq!(exposed_save_byte(3), 0xB3);
    assert!(lock(&crate::sram::SRAM).is_published());
    assert_stable_memory(stable);
}

#[test]
fn abi_deinit_without_unload_consumes_dirty_save_ram_and_clears_memory() {
    let _session = AbiSession::new();
    load_rom(&gba_sram_rom(), "deinit.gba");
    write_save_byte(5, 0xD5);
    assert!(!lock(&crate::sram::SRAM).is_published());

    crate::retro_deinit();

    assert!(lock(&CORE).is_none());
    let (data, size) = save_memory();
    assert!(data.is_null());
    assert_eq!(size, 0);
}

#[test]
fn abi_unload_clears_core_after_save_ram_sync_failure() {
    let _session = AbiSession::new();
    load_rom(&gba_sram_rom(), "unload-failure.gba");
    lock(&crate::sram::SRAM).truncate_exposed(1);

    crate::game::retro_unload_game();

    assert!(lock(&CORE).is_none());
    assert_eq!(
        crate::memory::retro_get_memory_size(RETRO_MEMORY_SAVE_RAM),
        1
    );
    assert!(!lock(&crate::sram::SRAM).is_published());
}
