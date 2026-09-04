use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use winit::event_loop::ActiveEventLoop;

use super::App;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BrowserAppCase {
    #[default]
    Sms,
    Gba,
}

impl BrowserAppCase {
    fn active_system(self) -> crate::emu_backend::ActiveSystem {
        match self {
            Self::Sms => crate::emu_backend::ActiveSystem::MasterSystem,
            Self::Gba => crate::emu_backend::ActiveSystem::GameBoyAdvance,
        }
    }

    fn perf_platform(self) -> &'static str {
        match self {
            Self::Sms => "Sega 8-bit",
            Self::Gba => "GBA",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProbePhase {
    #[default]
    WaitingForRom,
    SyncingLayerPolicy,
    Armed,
    Retiring,
    TerminalSeen,
    ExitRequested,
    Complete,
}

#[derive(Default)]
struct BrowserAppProbe {
    case: BrowserAppCase,
    phase: ProbePhase,
    canvas_ready: bool,
    drop_dispatched: bool,
    gfx_seen: bool,
    system_seen: bool,
    thread_seen: bool,
    retired_threads: usize,
    last_rendered: bool,
    last_frames_in_flight: usize,
    detached_runs: usize,
    committed_frames: usize,
    baseline_detached_runs: usize,
    baseline_committed_frames: usize,
    frames_to_step: usize,
    debug_request_pending: bool,
    debug_actions_pending: bool,
    remote_capture_pending: bool,
    performance_tab_open: bool,
    perf_requirement: bool,
    present_count: u64,
    uploaded_frame: Option<Vec<u8>>,
    upload_present_count: u64,
    frame_results: usize,
    advanced_frames: usize,
    runtime_fault_seen: bool,
    failure: Option<String>,
}

thread_local! {
    static PROBE: RefCell<BrowserAppProbe> = RefCell::new(BrowserAppProbe::default());
}

const SMS_BROWSER_ROM: &[u8] = &[
    0x3E, 0x80, 0xD3, 0x7F, 0x3E, 0x04, 0xD3, 0x7F, 0x3E, 0x90, 0xD3, 0x7F, 0x3E, 0x04, 0xD3, 0xBF,
    0x3E, 0x80, 0xD3, 0xBF, 0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, 0x3E, 0x00, 0xD3, 0xBF,
    0x3E, 0xC0, 0xD3, 0xBF, 0x3E, 0x03, 0xD3, 0xBE, 0x06, 0x30, 0xDB, 0xBF, 0xE6, 0x80, 0x28, 0xFA,
    0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, 0x78, 0xD3, 0xBE, 0x78, 0xEE, 0x33, 0x47, 0x18,
    0xE9,
];

const GBA_BROWSER_PROGRAM: &[u32] = &[
    0xE3A0_0301,
    0xE3A0_1B01,
    0xE381_1003,
    0xE1C0_10B0,
    0xE3A0_2406,
    0xE3A0_301F,
    0xE1D0_10B6,
    0xE351_00A0,
    0x3AFF_FFFC,
    0xE1C2_30B0,
    0xE223_300F,
    0xE1D0_10B6,
    0xE351_00A0,
    0x2AFF_FFFC,
    0xEAFF_FFF6,
];

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
let browserAppTestCompletion = null;

export function browser_app_test_delay(milliseconds) {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

export function begin_browser_app_test_completion(timeoutMilliseconds) {
    if (browserAppTestCompletion !== null) {
        throw new Error('browser App test completion is already registered');
    }
    return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
            browserAppTestCompletion = null;
            reject(new Error('browser App lifecycle timed out'));
        }, timeoutMilliseconds);
        browserAppTestCompletion = (error) => {
            clearTimeout(timeout);
            browserAppTestCompletion = null;
            if (error.length === 0) {
                resolve();
            } else {
                reject(new Error(error));
            }
        };
    });
}

export function complete_browser_app_test(error) {
    if (browserAppTestCompletion !== null) {
        browserAppTestCompletion(error);
    }
}
")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch)]
    async fn browser_app_test_delay(
        milliseconds: u32,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;

    fn begin_browser_app_test_completion(timeout_milliseconds: u32) -> js_sys::Promise;

    fn complete_browser_app_test(error: &str);
}

pub(super) fn drive_app(app: &mut App, event_loop: &ActiveEventLoop) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let expected_system = probe.case.active_system();
        probe.gfx_seen |= app.gfx.is_some();
        probe.system_seen |= app.active_system == expected_system;
        probe.thread_seen |= app.emu_thread.is_some();
        probe.retired_threads = app.wasm_retired_threads.len();
    });
    let phase = PROBE.with(|probe| probe.borrow().phase);
    if phase == ProbePhase::TerminalSeen {
        if !app.wasm_retired_threads.is_empty() {
            return;
        }
        PROBE.with(|probe| probe.borrow_mut().phase = ProbePhase::ExitRequested);
        event_loop.exit();
        return;
    }
    if matches!(phase, ProbePhase::ExitRequested | ProbePhase::Complete) {
        event_loop.exit();
        return;
    }
    if phase == ProbePhase::SyncingLayerPolicy {
        if app.frames_in_flight != 0
            || app.debug_requests.has_pending()
            || app.pending_debug_actions.has_pending()
        {
            return;
        }
        let counts = app
            .emu_thread
            .as_ref()
            .expect("syncing browser App thread")
            .speculation_counts_for_browser_test();
        if counts != (0, 1) {
            finish_with_failure(format!(
                "unexpected layer-policy baseline: {}/{}",
                counts.0, counts.1
            ));
            event_loop.exit();
            return;
        }
        app.debug_requests.frame_advance = true;
        app.debug_dock = egui_dock::DockState::new(vec![crate::debug::DebugTab::Performance]);
        app.emu_thread
            .as_ref()
            .expect("browser App thread")
            .force_detached_frame_for_browser_test();
        PROBE.with(|probe| {
            let mut probe = probe.borrow_mut();
            probe.phase = ProbePhase::Armed;
            probe.baseline_detached_runs = counts.0;
            probe.baseline_committed_frames = counts.1;
            probe.uploaded_frame = None;
            probe.upload_present_count = probe.present_count;
        });
        return;
    }
    if phase != ProbePhase::WaitingForRom || app.gfx.is_none() || app.emu_thread.is_none() {
        return;
    }

    let expected_system = PROBE.with(|probe| probe.borrow().case.active_system());
    if app.active_system != expected_system {
        finish_with_failure(format!(
            "dropped ROM created {:?}, expected {expected_system:?}",
            app.active_system,
        ));
        event_loop.exit();
        return;
    }

    app.set_user_paused(true);
    app.settings.ui.show_fps = false;
    PROBE.with(|probe| probe.borrow_mut().phase = ProbePhase::SyncingLayerPolicy);
}

pub(super) fn observe_app_tick(app: &mut App, rendered: bool) {
    let counts = app.emu_thread.as_ref().map_or((0, 0), |thread| {
        thread.speculation_counts_for_browser_test()
    });
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        probe.last_rendered = rendered;
        probe.last_frames_in_flight = app.frames_in_flight;
        probe.detached_runs = counts.0;
        probe.committed_frames = counts.1;
    });
    let armed = PROBE.with(|probe| probe.borrow().phase == ProbePhase::Armed);
    if !armed || !rendered || app.frames_in_flight != 0 {
        return;
    }

    let Some(thread) = app.emu_thread.as_ref() else {
        return;
    };
    let (completed_runs, committed_frames) = counts;
    let (baseline_detached_runs, baseline_committed_frames) = PROBE.with(|probe| {
        let probe = probe.borrow();
        (
            probe.baseline_detached_runs,
            probe.baseline_committed_frames,
        )
    });
    if completed_runs <= baseline_detached_runs || committed_frames <= baseline_committed_frames {
        return;
    }

    let validation = (|| -> Result<(), String> {
        let (frame_results, advanced_frames, runtime_fault_seen) = PROBE.with(|probe| {
            let probe = probe.borrow();
            (
                probe.frame_results,
                probe.advanced_frames,
                probe.runtime_fault_seen,
            )
        });
        if frame_results != 1 || advanced_frames != 1 || runtime_fault_seen {
            return Err(format!(
                "unexpected App FrameResult: count={frame_results} advanced={advanced_frames} fault={runtime_fault_seen}"
            ));
        }
        let expected_counts = (baseline_detached_runs + 1, baseline_committed_frames + 1);
        if (completed_runs, committed_frames) != expected_counts {
            return Err(format!(
                "unexpected detached/commit counts: {completed_runs}/{committed_frames}, expected {}/{}",
                expected_counts.0, expected_counts.1
            ));
        }
        let displayed = app
            .last_displayed_frame
            .as_deref()
            .ok_or_else(|| "App did not retain a displayed frame".to_string())?;
        if displayed.is_empty() {
            return Err("App displayed an empty framebuffer".to_string());
        }
        let selected = thread
            .shared_framebuffer()
            .load_full()
            .ok_or_else(|| "worker did not publish a selected framebuffer".to_string())?;
        let primary = thread.primary_framebuffer_for_browser_test();
        if selected.as_slice() == primary.as_slice() {
            return Err(
                "visual fixture did not distinguish primary and projected frames".to_string(),
            );
        }
        if selected.as_slice() != displayed.as_slice() {
            return Err("App display does not match the worker-selected framebuffer".to_string());
        }
        let (uploaded, upload_present_count, present_count) = PROBE.with(|probe| {
            let probe = probe.borrow();
            (
                probe.uploaded_frame.clone(),
                probe.upload_present_count,
                probe.present_count,
            )
        });
        let uploaded = uploaded.ok_or_else(|| "Graphics upload was not observed".to_string())?;
        if uploaded.as_slice() != displayed.as_slice() {
            return Err("Graphics upload does not match the selected framebuffer".to_string());
        }
        if present_count <= upload_present_count {
            return Err(
                "selected framebuffer upload was not followed by surface present".to_string(),
            );
        }
        let perf = app
            .cached_ui_data
            .as_ref()
            .and_then(|data| data.perf_info.as_ref())
            .ok_or_else(|| "App did not consume requested UiFrameData".to_string())?;
        let expected_platform = PROBE.with(|probe| probe.borrow().case.perf_platform());
        if perf.platform_name != expected_platform {
            return Err(format!(
                "unexpected UiFrameData platform: {}, expected {expected_platform}",
                perf.platform_name,
            ));
        }
        if perf.frames_in_flight != 0 || perf.speed_mode_label != "Paused" {
            return Err(format!(
                "App did not finalize UiFrameData: in_flight={} speed={}",
                perf.frames_in_flight, perf.speed_mode_label
            ));
        }
        Ok(())
    })();

    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        if let Err(error) = validation {
            probe.failure = Some(error);
        }
        probe.phase = ProbePhase::Retiring;
    });
    app.stop_emu_thread_for_user_stop();
}

pub(super) fn observe_retired_shutdown(thread: &crate::emu_thread::EmuThread) {
    let counts = thread.speculation_counts_for_browser_test();
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let expected_counts = (
            probe.baseline_detached_runs + 1,
            probe.baseline_committed_frames + 1,
        );
        if probe.phase != ProbePhase::Retiring {
            probe.failure = Some("retired worker completed before App verification".to_string());
        } else if counts != expected_counts {
            probe.failure = Some(format!(
                "terminal detached/commit counts changed: {}/{}, expected {}/{}",
                counts.0, counts.1, expected_counts.0, expected_counts.1
            ));
        }
        probe.phase = ProbePhase::TerminalSeen;
    });
}

pub(super) fn record_frame_result(app: &mut App, result: &crate::emu_thread::FrameResult) {
    let first_meaningful = PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let meaningful = probe.phase == ProbePhase::Armed && result.advanced_frames != 0;
        let first_meaningful = meaningful && probe.frame_results == 0;
        if meaningful {
            probe.frame_results += 1;
            probe.advanced_frames += result.advanced_frames;
        }
        if matches!(
            probe.phase,
            ProbePhase::SyncingLayerPolicy | ProbePhase::Armed
        ) {
            probe.runtime_fault_seen |= result.runtime_fault.is_some();
        }
        first_meaningful
    });
    if !first_meaningful {
        return;
    }
    let Some(location) = app
        .debug_dock
        .find_tab(&crate::debug::DebugTab::Performance)
    else {
        finish_with_failure("one-shot Performance tab disappeared before result".to_string());
        return;
    };
    app.debug_dock.remove_tab(location);
}

pub(super) fn record_tick_request_state(
    frames_to_step: usize,
    debug_request_pending: bool,
    debug_actions_pending: bool,
    remote_capture_pending: bool,
    performance_tab_open: bool,
    perf_requirement: bool,
) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        probe.frames_to_step = frames_to_step;
        probe.debug_request_pending = debug_request_pending;
        probe.debug_actions_pending = debug_actions_pending;
        probe.remote_capture_pending = remote_capture_pending;
        probe.performance_tab_open = performance_tab_open;
        probe.perf_requirement = perf_requirement;
    });
}

pub(super) fn record_app_exiting() {
    let error = PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        if !matches!(
            probe.phase,
            ProbePhase::ExitRequested | ProbePhase::Complete
        ) {
            probe.failure = Some("App exited before retired worker completion".to_string());
        }
        probe.phase = ProbePhase::Complete;
        probe.failure.clone().unwrap_or_default()
    });
    complete_browser_app_test(&error);
}

pub(crate) fn record_framebuffer_upload(framebuffer: &[u8]) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        if probe.phase == ProbePhase::Armed {
            probe.uploaded_frame = Some(framebuffer.to_vec());
            probe.upload_present_count = probe.present_count;
        }
    });
}

pub(crate) fn record_surface_present() {
    PROBE.with(|probe| {
        probe.borrow_mut().present_count += 1;
    });
}

fn finish_with_failure(error: String) {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        probe.failure = Some(error);
        probe.phase = ProbePhase::Complete;
    });
}

fn reset_probe(case: BrowserAppCase) {
    PROBE.with(|probe| {
        *probe.borrow_mut() = BrowserAppProbe {
            case,
            ..BrowserAppProbe::default()
        };
    });
}

fn probe_outcome() -> Option<Result<(), String>> {
    PROBE.with(|probe| {
        let probe = probe.borrow();
        (probe.phase == ProbePhase::Complete).then(|| probe.failure.clone().map_or(Ok(()), Err))
    })
}

fn probe_diagnostic() -> String {
    PROBE.with(|probe| {
        let probe = probe.borrow();
        format!(
            "case={:?} phase={:?} canvas={} drop={} gfx={} system={} thread={} retired={} rendered={} in_flight={} detached={} committed={} baseline={}/{} present={} uploaded={} frame_results={} advanced={} fault={} step={} debug_req={} debug_actions={} remote={} perf_tab={} perf_req={}",
            probe.case,
            probe.phase,
            probe.canvas_ready,
            probe.drop_dispatched,
            probe.gfx_seen,
            probe.system_seen,
            probe.thread_seen,
            probe.retired_threads,
            probe.last_rendered,
            probe.last_frames_in_flight,
            probe.detached_runs,
            probe.committed_frames,
            probe.baseline_detached_runs,
            probe.baseline_committed_frames,
            probe.present_count,
            probe.uploaded_frame.is_some(),
            probe.frame_results,
            probe.advanced_frames,
            probe.runtime_fault_seen,
            probe.frames_to_step,
            probe.debug_request_pending,
            probe.debug_actions_pending,
            probe.remote_capture_pending,
            probe.performance_tab_open,
            probe.perf_requirement
        )
    })
}

fn js_error_message(error: &wasm_bindgen::JsValue) -> String {
    js_sys::Reflect::get(error, &wasm_bindgen::JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "unknown JavaScript error".to_string())
}

async fn wait_for_canvas() -> web_sys::HtmlCanvasElement {
    let deadline = js_sys::Date::now() + 20_000.0;
    loop {
        let canvas = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.query_selector("canvas").ok().flatten())
            .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        if let Some(canvas) = canvas
            && canvas.is_connected()
            && canvas.width() != 0
            && canvas.height() != 0
        {
            PROBE.with(|probe| probe.borrow_mut().canvas_ready = true);
            return canvas;
        }
        assert!(
            js_sys::Date::now() < deadline,
            "App canvas creation timed out"
        );
        browser_app_test_delay(5)
            .await
            .expect("browser timer should resolve");
    }
}

fn dispatch_rom_drop(rom: &[u8], name: &str) {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(rom));
    let file = web_sys::File::new_with_u8_array_sequence(parts.as_ref(), name)
        .expect("browser File should construct");
    let transfer = web_sys::DataTransfer::new().expect("DataTransfer should construct");
    transfer
        .items()
        .add_with_file(&file)
        .expect("ROM should enter DataTransfer")
        .expect("DataTransfer should retain ROM");
    let init = web_sys::DragEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_data_transfer(Some(&transfer));
    let event = web_sys::DragEvent::new_with_event_init_dict("drop", &init)
        .expect("drop event should construct");
    web_sys::window()
        .expect("browser window")
        .document()
        .expect("browser document")
        .body()
        .expect("browser body")
        .dispatch_event(&event)
        .expect("drop event should dispatch");
    PROBE.with(|probe| probe.borrow_mut().drop_dispatched = true);
}

fn gba_browser_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    for (chunk, opcode) in rom
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(GBA_BROWSER_PROGRAM.iter().copied())
    {
        chunk.copy_from_slice(&opcode.to_le_bytes());
    }
    rom[0xA0..0xAB].copy_from_slice(b"BROWSER GBA");
    rom[0xAC..0xB0].copy_from_slice(b"AXVE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn frame_is_nonblack(framebuffer: &[u8]) -> bool {
    framebuffer
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[..3] != [0, 0, 0])
}

fn assert_sms_visual_fixture_changes_on_the_next_frame() {
    let mut emulator = zeff_sega8_core::emulator::Emulator::new_with_hint(
        SMS_BROWSER_ROM,
        48_000,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .expect("browser SMS fixture should load");
    emulator.step_frame();
    emulator.step_frame();
    let primary = emulator.framebuffer().to_vec();
    emulator.step_frame();
    let projected = emulator.framebuffer();
    assert!(
        frame_is_nonblack(&primary)
            && frame_is_nonblack(projected)
            && primary.as_slice() != projected,
        "browser SMS fixture frames 2 and 3 must be nonblack and unequal"
    );
}

fn assert_gba_visual_fixture_changes_on_the_next_frame(rom: &[u8]) {
    let mut emulator = zeff_gba_core::emulator::Emulator::new(rom, 48_000)
        .expect("browser GBA fixture should load");
    assert!(!emulator.save_ram_kind().has_ram());
    assert!(!emulator.has_rtc());
    emulator.step_frame();
    emulator.step_frame();
    let primary = emulator.framebuffer().to_vec();
    emulator.step_frame();
    let projected = emulator.framebuffer();
    assert_eq!(&primary[..4], &[0xFF, 0, 0, 0xFF]);
    assert_eq!(&projected[..4], &[0x84, 0, 0, 0xFF]);
    assert!(
        frame_is_nonblack(&primary)
            && frame_is_nonblack(projected)
            && primary.as_slice() != projected,
        "browser GBA fixture frames 2 and 3 must be nonblack and unequal"
    );
}

async fn run_browser_app_test(case: BrowserAppCase, rom: &[u8], name: &str) {
    crate::platform::clear_browser_storage_for_test()
        .await
        .expect("browser storage should clear");
    crate::platform::init_storage().await;
    reset_probe(case);
    let document = web_sys::window()
        .expect("browser window")
        .document()
        .expect("browser document");
    document
        .body()
        .expect("browser body")
        .set_attribute("style", "width:640px;height:480px;margin:0")
        .expect("browser body dimensions should apply");
    assert!(
        document
            .query_selector("canvas")
            .expect("canvas query should succeed")
            .is_none(),
        "dedicated App test realm should start without a canvas"
    );
    assert!(
        crate::platform::check_webgpu_support().await,
        "production WebGPU preflight should succeed"
    );
    let completion =
        wasm_bindgen_futures::JsFuture::from(begin_browser_app_test_completion(30_000));
    super::run(None, crate::settings::Settings::default()).expect("App should start");
    let canvas = wait_for_canvas().await;
    assert!(canvas.is_connected());
    assert!(canvas.width() != 0 && canvas.height() != 0);
    dispatch_rom_drop(rom, name);
    if let Err(error) = completion.await {
        panic!(
            "browser App lifecycle failed: {}; promise={}",
            probe_diagnostic(),
            js_error_message(&error)
        );
    }
    probe_outcome()
        .expect("browser App exiting callback should finalize the probe")
        .expect("browser App speculation proof failed");
}

#[wasm_bindgen_test]
async fn wasm_sms_browser_app_consumes_and_presents_detached_frame() {
    assert_sms_visual_fixture_changes_on_the_next_frame();
    run_browser_app_test(BrowserAppCase::Sms, SMS_BROWSER_ROM, "browser-app.sms").await;
}

#[wasm_bindgen_test]
async fn wasm_gba_browser_app_consumes_and_presents_detached_frame() {
    let rom = gba_browser_rom();
    assert_gba_visual_fixture_changes_on_the_next_frame(&rom);
    run_browser_app_test(BrowserAppCase::Gba, &rom, "browser-app.gba").await;
}
