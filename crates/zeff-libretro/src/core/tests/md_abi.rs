use super::*;
use crate::api::*;
use crate::callbacks::*;
use std::ffi::{CStr, CString};
use std::os::raw::{c_uint, c_void};
use std::sync::Mutex;
use zeff_md_core::{ControllerKind, ControllerPort};

#[derive(Default)]
struct Observations {
    controller_info: usize,
    input_descriptors: usize,
    video: Option<(u32, u32, usize)>,
    nonzero_audio: bool,
    av: Vec<(u32, u32, f32, f64)>,
    pressed: Option<(u32, u32)>,
    can_dupe: Option<bool>,
    video_calls: usize,
    duplicate_geometry: Option<(u32, u32, usize)>,
    audio_frames: usize,
    polls: usize,
}

static OBSERVED: Mutex<Observations> = Mutex::new(Observations {
    controller_info: 0,
    input_descriptors: 0,
    video: None,
    nonzero_audio: false,
    av: Vec::new(),
    pressed: None,
    can_dupe: None,
    video_calls: 0,
    duplicate_geometry: None,
    audio_frames: 0,
    polls: 0,
});

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        RETRO_ENVIRONMENT_GET_CAN_DUPE => {
            let Some(supported) = lock(&OBSERVED).can_dupe else {
                return false;
            };
            unsafe {
                *data.cast::<bool>() = supported;
            }
            true
        }
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => unsafe {
            *(data.cast::<u32>()) == RETRO_PIXEL_FORMAT_XRGB8888
        },
        RETRO_ENVIRONMENT_SET_CONTROLLER_INFO => {
            lock(&OBSERVED).controller_info = data as usize;
            true
        }
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
            lock(&OBSERVED).input_descriptors = data as usize;
            true
        }
        RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO => {
            let av = unsafe { &*data.cast::<retro_system_av_info>() };
            lock(&OBSERVED).av.push((
                av.geometry.base_width,
                av.geometry.base_height,
                av.geometry.aspect_ratio,
                av.timing.fps,
            ));
            true
        }
        _ => false,
    }
}

unsafe extern "C" fn video(data: *const c_void, width: u32, height: u32, pitch: usize) {
    assert!(!data.is_null());
    lock(&OBSERVED).video = Some((width, height, pitch));
}

unsafe extern "C" fn audio(data: *const i16, frames: usize) -> usize {
    let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
    lock(&OBSERVED).nonzero_audio |= samples.iter().any(|sample| *sample != 0);
    lock(&OBSERVED).audio_frames += frames;
    frames
}

unsafe extern "C" fn input(port: u32, device: u32, _index: u32, id: u32) -> i16 {
    assert_eq!(device, RETRO_DEVICE_JOYPAD);
    i16::from(lock(&OBSERVED).pressed == Some((port, id)))
}

struct Session {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Session {
    fn new() -> Self {
        let guard = lock(&ABI_TEST_LOCK);
        clear();
        *lock(&CB_ENVIRONMENT) = Some(environment);
        crate::retro_set_video_refresh(video);
        *lock(&CB_AUDIO_SAMPLE_BATCH) = Some(audio);
        *lock(&CB_INPUT_STATE) = Some(input);
        Self { _guard: guard }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        clear();
    }
}

fn clear() {
    *lock(&CORE) = None;
    *lock(&CB_ENVIRONMENT) = None;
    lock(&CB_VIDEO_REFRESH).install(None);
    *lock(&CB_AUDIO_SAMPLE_BATCH) = None;
    *lock(&CB_INPUT_STATE) = None;
    *lock(&CB_INPUT_POLL) = None;
    *lock(&MAX_SERIALIZE_SIZE) = 0;
    *lock(&FRAME_COUNTER) = 0;
    lock(&crate::sram::SRAM).clear();
    *lock(&OBSERVED) = Observations::default();
}

fn load(bytes: &[u8], path: &str) -> bool {
    let path = CString::new(path).unwrap();
    let info = retro_game_info {
        path: path.as_ptr(),
        data: bytes.as_ptr().cast(),
        size: bytes.len(),
        meta: std::ptr::null(),
    };
    crate::game::retro_load_game(&info)
}

fn machine(state: &CoreState) -> &zeff_md_core::Emulator {
    let ActiveCore::Md(emu) = &state.core else {
        panic!("expected MD core");
    };
    emu
}

#[test]
fn md_abi_controller_types_have_durable_backing_and_poll_all_buttons_on_both_ports() {
    let _session = Session::new();
    assert!(load(&md::rom(b'U', &[0x60fe, 0x4e71], false), "input.md"));
    let info_ptr = lock(&OBSERVED).controller_info as *const retro_controller_info;
    assert!(!info_ptr.is_null());
    for index in 0..2 {
        let info = unsafe { &*info_ptr.add(index) };
        assert_eq!(info.num_types, 2);
        let types = unsafe { std::slice::from_raw_parts(info.types, 2) };
        assert_eq!(types[0].id, RETRO_DEVICE_JOYPAD);
        assert_eq!(types[1].id, crate::input::MD_SIX_BUTTON_DEVICE);
        assert_eq!(
            unsafe { CStr::from_ptr(types[1].desc) }.to_str().unwrap(),
            "MD 6-button pad"
        );
    }
    let descriptors = lock(&OBSERVED).input_descriptors as *const retro_input_descriptor;
    for (offset, port, id, label) in [
        (0, 0, RETRO_DEVICE_ID_JOYPAD_Y, "A"),
        (2, 0, RETRO_DEVICE_ID_JOYPAD_A, "C"),
        (12, 1, RETRO_DEVICE_ID_JOYPAD_Y, "A"),
    ] {
        let descriptor = unsafe { &*descriptors.add(offset) };
        assert_eq!((descriptor.port, descriptor.id), (port, id));
        assert_eq!(
            unsafe { CStr::from_ptr(descriptor.description) }
                .to_str()
                .unwrap(),
            label
        );
    }
    for port in 0..2 {
        {
            let core = lock(&CORE);
            assert_eq!(
                machine(core.as_ref().unwrap())
                    .input()
                    .controller_kind([ControllerPort::One, ControllerPort::Two][port as usize]),
                ControllerKind::ThreeButton
            );
        }
        crate::retro_set_controller_port_device(port, crate::input::MD_SIX_BUTTON_DEVICE);
        for (id, raw, extra) in [
            (RETRO_DEVICE_ID_JOYPAD_Y, 0xbf, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_B, 0xef, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_A, 0xdf, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_L, 0xff, 0x0b),
            (RETRO_DEVICE_ID_JOYPAD_X, 0xff, 0x0d),
            (RETRO_DEVICE_ID_JOYPAD_R, 0xff, 0x0e),
            (RETRO_DEVICE_ID_JOYPAD_SELECT, 0xff, 0x07),
            (RETRO_DEVICE_ID_JOYPAD_START, 0x7f, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_UP, 0xfe, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_DOWN, 0xfd, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_LEFT, 0xfb, 0x0f),
            (RETRO_DEVICE_ID_JOYPAD_RIGHT, 0xf7, 0x0f),
        ] {
            lock(&OBSERVED).pressed = Some((port, id));
            let mut core = lock(&CORE);
            let state = core.as_mut().unwrap();
            assert!(state.poll_md_input());
            let input = machine(state).input();
            let controller = [ControllerPort::One, ControllerPort::Two][port as usize];
            assert_eq!(input.controller_kind(controller), ControllerKind::SixButton);
            assert_eq!(input.controller_raw(controller), raw);
            assert_eq!(input.controller_extra_raw(controller), extra);
        }
        crate::retro_set_controller_port_device(port, RETRO_DEVICE_NONE);
        let mut core = lock(&CORE);
        let state = core.as_mut().unwrap();
        state.poll_md_input();
        let controller = [ControllerPort::One, ControllerPort::Two][port as usize];
        assert_eq!(machine(state).input().controller_raw(controller), 0xff);
        assert_eq!(
            machine(state).input().controller_extra_raw(controller),
            0x0f
        );
    }
}

#[test]
fn md_abi_runs_serializes_restores_external_save_and_preserves_failed_load_session() {
    let _session = Session::new();
    let guest = md::guest();
    assert!(load(&guest, "guest.md"));
    crate::retro_run();
    assert_eq!(lock(&OBSERVED).video, Some((256, 224, 256 * 4)));
    assert!(lock(&OBSERVED).nonzero_audio);
    assert_eq!(
        crate::memory::retro_get_memory_size(RETRO_MEMORY_SAVE_RAM),
        8192
    );
    assert_eq!(
        crate::memory::retro_get_memory_size(RETRO_MEMORY_SYSTEM_RAM),
        65536
    );
    assert_eq!(
        crate::memory::retro_get_memory_size(RETRO_MEMORY_VIDEO_RAM),
        65536
    );
    let save = crate::memory::retro_get_memory_data(RETRO_MEMORY_SAVE_RAM).cast::<u8>();
    assert!(!save.is_null());
    unsafe {
        save.add(3).write(0x7b);
    }
    let size = crate::serialization::retro_serialize_size();
    assert_eq!(size, zeff_md_core::MAX_FRONTEND_STATE_BYTES + 4100);
    let mut serialized = vec![0; size];
    assert!(crate::serialization::retro_serialize(
        serialized.as_mut_ptr().cast(),
        size
    ));
    let expected = lock(&CORE).as_ref().unwrap().encode_state().unwrap();
    assert!(!load(&[0; 7], "invalid.md"));
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        expected
    );
    let mut rejected = serialized.clone();
    rejected[4] ^= 1;
    assert!(!crate::serialization::retro_unserialize(
        rejected.as_ptr().cast(),
        size
    ));
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        expected
    );
    unsafe {
        save.add(3).write(0x19);
    }
    crate::retro_reset();
    assert_eq!(unsafe { save.add(3).read() }, 0x19);
    assert!(crate::serialization::retro_unserialize(
        serialized.as_ptr().cast(),
        size
    ));
    assert_eq!(unsafe { save.add(3).read() }, 0x7b);
    assert_eq!(
        lock(&CORE).as_ref().unwrap().encode_state().unwrap(),
        expected
    );
    crate::retro_run();
    assert_eq!(unsafe { save.read() }, 0xa5);
}

#[test]
fn md_abi_indexed_cheats_disable_and_replace_only_the_addressed_slot() {
    let _session = Session::new();
    let bytes = md::rom(
        b'U',
        &[
            0x13fc, 0x11, 0xff, 0x100, 0x33fc, 0x1122, 0xff, 0x110, 0x13f9, 0xff, 0x100, 0xff,
            0x101, 0x33f9, 0xff, 0x110, 0xff, 0x112, 0x33f9, 0, 0x100, 0xff, 0x114, 0x60d0,
        ],
        false,
    );
    assert!(load(&bytes, "indexed.md"));
    let snapshot = || {
        crate::retro_run();
        crate::retro_run();
        let data = crate::memory::retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM).cast::<u8>();
        assert!(!data.is_null());
        let ram = unsafe { std::slice::from_raw_parts(data, 65536) };
        (
            ram[0x100],
            ram[0x102],
            [ram[0x110], ram[0x111]],
            [ram[0x114], ram[0x115]],
        )
    };
    let mixed = c"FF0100:A5+000100:A55A";
    assert_eq!(snapshot(), (0x11, 0, [0x11, 0x22], [0x53, 0x45]));
    crate::retro_cheat_set(3, true, mixed.as_ptr());
    crate::retro_cheat_set(u32::MAX, true, c"FF0110:0008".as_ptr());
    assert_eq!(snapshot(), (0xa5, 0, [0, 8], [0xa5, 0x5a]));
    crate::retro_cheat_set(3, false, std::ptr::null());
    assert_eq!(snapshot(), (0x11, 0, [0, 8], [0x53, 0x45]));

    crate::retro_cheat_set(3, true, mixed.as_ptr());
    crate::retro_cheat_set(3, true, c"008000:1234+FF0102:55".as_ptr());
    assert_eq!(snapshot(), (0xa5, 0, [0, 8], [0xa5, 0x5a]));
    crate::retro_cheat_set(3, true, c"FF0102:77".as_ptr());
    assert_eq!(snapshot(), (0x11, 0x77, [0, 8], [0x53, 0x45]));
    crate::retro_reset();
    assert_eq!(snapshot(), (0x11, 0x77, [0, 8], [0x53, 0x45]));

    crate::retro_cheat_reset();
    crate::retro_reset();
    assert_eq!(snapshot(), (0x11, 0, [0x11, 0x22], [0x53, 0x45]));
    crate::retro_cheat_set(10, true, c"000005:00+FF0100:A5".as_ptr());
    crate::retro_cheat_set(9, true, c"000005:08+FF0102:77".as_ptr());
    crate::retro_cheat_set(11, true, c"000100:A55A+FF0110:0008".as_ptr());
    assert_eq!(snapshot(), (0xa5, 0x77, [0, 8], [0xa5, 0x5a]));
    crate::retro_cheat_set(10, false, std::ptr::null());
    assert_eq!(snapshot(), (0x11, 0x77, [0, 8], [0xa5, 0x5a]));
    crate::retro_cheat_set(3, true, c"FF0100:99".as_ptr());
    assert_eq!(snapshot(), (0x99, 0x77, [0, 8], [0xa5, 0x5a]));
}

#[test]
fn md_abi_publishes_actual_mode2_geometry_and_field_cadence() {
    unknown_cram_callback_protocol();
    let _session = Session::new();
    assert!(load(&md::interlace_rom(), "pal.gen"));
    assert_eq!(crate::retro_get_region(), RETRO_REGION_PAL);
    for _ in 0..3 {
        crate::retro_run();
    }
    assert_eq!(lock(&OBSERVED).video, Some((320, 480, 320 * 4)));
    let mut av = std::mem::MaybeUninit::uninit();
    crate::retro_get_system_av_info(av.as_mut_ptr());
    let av = unsafe { av.assume_init() };
    assert_eq!(
        (av.geometry.base_width, av.geometry.base_height),
        (320, 480)
    );
    assert_eq!((av.geometry.max_width, av.geometry.max_height), (320, 480));
    assert_eq!(av.geometry.aspect_ratio, 4.0 / 3.0);
    let core = lock(&CORE);
    let state = core.as_ref().unwrap();
    assert_eq!(
        lock(&OBSERVED).av.last().copied(),
        Some((320, 480, av.geometry.aspect_ratio, state.fps()))
    );
    assert_eq!(machine(state).pacing_field_count(), 3);
}

fn unknown_cram_callback_protocol() {
    unsafe extern "C" fn observed_video(
        data: *const c_void,
        width: u32,
        height: u32,
        pitch: usize,
    ) {
        let mut observed = lock(&OBSERVED);
        if data.is_null() {
            observed.duplicate_geometry = Some((width, height, pitch));
        } else {
            observed.video_calls += 1;
            observed.video = Some((width, height, pitch));
        }
    }
    unsafe extern "C" fn poll() {
        lock(&OBSERVED).polls += 1;
    }
    let mut words = Vec::new();
    for value in [0x8114, 0x8f02, 0x9301, 0x9780, 0xc07e, 0x0080] {
        words.extend([0x33fc, value, 0x00c0, 4]);
    }
    words.extend([0x33fc, 0xabcd, 0x00c0, 0]);
    words.extend([0x1039, 0x00a1, 3, 0x0200, 2, 0x66f4]);
    for (address, value) in [
        (0xc00004, 0xc000),
        (0xc00004, 0),
        (0xc00000, 0x0246),
        (0xc00004, 0x8c81),
    ] {
        words.extend([0x33fc, value, (address >> 16) as u16, address as u16]);
    }
    words.extend([0x60fe, 0x4e71]);
    let bytes = md::rom(b'U', &words, false);
    for xrgb in [false, true] {
        for support in [Some(true), Some(false), None] {
            let _session = Session::new();
            lock(&OBSERVED).can_dupe = support;
            crate::retro_set_video_refresh(observed_video);
            *lock(&CB_INPUT_POLL) = Some(poll);
            assert!(load(&bytes, "cram.md"));
            USE_XRGB8888.store(xrgb, std::sync::atomic::Ordering::Relaxed);
            for field in 1..=2 {
                let before = lock(&OBSERVED).audio_frames;
                crate::retro_run();
                let core = lock(&CORE);
                let state = core.as_ref().unwrap();
                assert!(state.md_runtime_fault.is_none());
                assert_eq!(machine(state).pacing_field_count(), field);
                assert!(!state.framebuffer_available());
                assert!(state.last_presented_video.is_none());
                assert!(state.xrgb_buf.is_empty() && state.rgb565_buf.is_empty());
                let observed = lock(&OBSERVED);
                assert_eq!(observed.video_calls, 0);
                assert_eq!(observed.duplicate_geometry, None);
                assert!(observed.audio_frames > before);
                assert_eq!(observed.polls as u64, field);
            }
            let unavailable = lock(&CORE).as_ref().unwrap().encode_state().unwrap();
            lock(&OBSERVED).pressed = Some((0, RETRO_DEVICE_ID_JOYPAD_DOWN));
            for _ in 0..3 {
                crate::retro_run();
            }
            lock(&OBSERVED).pressed = None;
            let calls = lock(&OBSERVED).video_calls;
            assert!(calls > 0);
            assert_eq!(
                lock(&OBSERVED).video,
                Some((320, 224, 320 * if xrgb { 4 } else { 2 }))
            );
            let (xrgb_before, rgb565_before) = {
                let mut core = lock(&CORE);
                let state = core.as_mut().unwrap();
                state.load_state(&unavailable).unwrap();
                assert_eq!(
                    state
                        .last_presented_video
                        .map(|(_, width, height)| (width, height)),
                    Some((320, 224))
                );
                assert_eq!(state.video_geometry().base_width, 256);
                (state.xrgb_buf.clone(), state.rgb565_buf.clone())
            };
            let before = lock(&OBSERVED).audio_frames;
            crate::retro_run();
            let core = lock(&CORE);
            let state = core.as_ref().unwrap();
            assert!(state.md_runtime_fault.is_none());
            assert_eq!(machine(state).pacing_field_count(), 3);
            assert!(!state.framebuffer_available());
            assert_eq!(state.xrgb_buf, xrgb_before);
            assert_eq!(state.rgb565_buf, rgb565_before);
            let observed = lock(&OBSERVED);
            assert_eq!(observed.video_calls, calls);
            assert_eq!(
                observed.duplicate_geometry,
                (support == Some(true)).then_some((320, 224, 0))
            );
            assert!(observed.audio_frames > before);
            assert_eq!(observed.polls, 6);
            drop(observed);
            drop(core);
            let generation = lock(&CB_VIDEO_REFRESH).snapshot().1;
            crate::retro_set_video_refresh(observed_video);
            assert_ne!(lock(&CB_VIDEO_REFRESH).snapshot().1, generation);
            let before = {
                let mut observed = lock(&OBSERVED);
                observed.duplicate_geometry = None;
                observed.audio_frames
            };
            crate::retro_run();
            let core = lock(&CORE);
            let state = core.as_ref().unwrap();
            assert!(state.md_runtime_fault.is_none());
            assert_eq!(machine(state).pacing_field_count(), 4);
            assert!(!state.framebuffer_available());
            assert!(state.last_presented_video.is_none());
            assert_eq!(state.xrgb_buf, xrgb_before);
            assert_eq!(state.rgb565_buf, rgb565_before);
            let observed = lock(&OBSERVED);
            assert_eq!(observed.video_calls, calls);
            assert_eq!(observed.duplicate_geometry, None);
            assert!(observed.audio_frames > before);
            assert_eq!(observed.polls, 7);
        }
    }
}

#[path = "md_abi/frontend.rs"]
mod frontend;
