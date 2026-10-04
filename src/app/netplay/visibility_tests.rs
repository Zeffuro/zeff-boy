use super::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function visibility_test_yield() { return new Promise(resolve => setTimeout(resolve, 1)); }"
)]
extern "C" {
    async fn visibility_test_yield();
}

fn app(proxy: winit::event_loop::EventLoopProxy<()>, bytes: &[u8]) -> App {
    let mut settings = crate::settings::Settings::default();
    settings.audio.output_sample_rate = 48_000;
    settings.emulation.save_recovery_state = false;
    let mut app = crate::app::construct::create(None, settings, false, proxy);
    app.load_rom_from_bytes("browser-visibility.nes".into(), bytes.to_vec());
    app.debug_windows.netplay.lobby_url = option_env!("ZEFF_BROWSER_TEST_LOBBY_URL")
        .unwrap_or("ws://127.0.0.1:47180/v1/ws")
        .into();
    app.debug_windows.netplay.input_delay = 0;
    app
}

fn drain(app: &mut App) -> usize {
    let mut flushes = 0;
    while let Some(response) = app
        .emu_thread
        .as_ref()
        .and_then(|thread| thread.try_recv_response())
    {
        match &response {
            EmuResponse::SramFlushed(_) => flushes += 1,
            EmuResponse::SramFlushFailed(error) => panic!("{error}"),
            EmuResponse::Netplay(crate::netplay::Response::Frame { ports, .. }) => {
                assert_eq!(*ports, [0, 0x81]);
            }
            _ => {}
        }
        app.consume_netplay_response(response);
    }
    flushes
}

fn visible(app: &mut App, value: bool) {
    app.wasm_tab_visible.set(value);
    app.check_tab_visibility();
}

fn hold(app: &mut App) {
    app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
}

async fn hidden_without_flush(app: &mut App) {
    visible(app, true);
    visible(app, false);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(100) {
        assert_eq!(drain(app), 0, "hidden netplay issued a battery flush");
        assert!(app.netplay.fenced());
        assert!(!matches!(
            app.netplay.phase,
            Phase::Stopping | Phase::Poisoned
        ));
        visibility_test_yield().await;
    }
}

async fn connect(host: &mut App, guest: &mut App) {
    let started = Instant::now();
    while !host.netplay.running() || !guest.netplay.running() {
        for app in [&mut *host, &mut *guest] {
            app.poll_retired_wasm_threads();
            app.pump_netplay();
            drain(app);
            hold(app);
            assert!(app.netplay.fenced(), "{}", app.debug_windows.netplay.status);
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        visibility_test_yield().await;
    }
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_hidden_tab_preserves_app_connection_and_local_flush() {
    crate::platform::init_storage().await;
    let global = js_sys::global();
    let name = wasm_bindgen::JsValue::from_str("zeffBoyBundleIdentity");
    let previous = js_sys::Reflect::get(&global, &name).unwrap();
    js_sys::Reflect::set(&global, &name, &const_hex::encode([7; 32]).into()).unwrap();
    let event_loop = winit::event_loop::EventLoop::<()>::with_user_event()
        .build()
        .unwrap();
    let mut bytes = zeff_netplay::fixture::rom();
    bytes[7] = 0x28;
    bytes[10] = 0x70;
    let mut host = app(event_loop.create_proxy(), &bytes);
    let mut guest = app(event_loop.create_proxy(), &bytes);
    drain(&mut host);
    drain(&mut guest);

    visible(&mut host, false);
    let started = Instant::now();
    let mut flushes = 0;
    while flushes == 0 {
        flushes += drain(&mut host);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "local visibility flush did not complete"
        );
        visibility_test_yield().await;
    }
    assert!(!host.netplay.fenced());
    assert!(
        crate::platform::read_sram_data(
            std::path::Path::new("browser-visibility.sav"),
            "nes",
            zeff_firmware::sha256_bytes(&bytes),
            "sram"
        )
        .unwrap()
        .is_some()
    );

    visible(&mut host, true);
    host.begin_netplay(None).unwrap();
    assert!(matches!(host.netplay.phase, Phase::Preparing(_)));
    hidden_without_flush(&mut host).await;
    let started = Instant::now();
    while host.debug_windows.netplay.invitation.is_empty() {
        host.poll_retired_wasm_threads();
        host.pump_netplay();
        drain(&mut host);
        assert!(
            host.netplay.fenced(),
            "{}",
            host.debug_windows.netplay.status
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        visibility_test_yield().await;
    }
    assert!(matches!(host.netplay.phase, Phase::Connecting));
    hidden_without_flush(&mut host).await;
    guest
        .begin_netplay(Some(host.debug_windows.netplay.invitation.clone()))
        .unwrap();
    connect(&mut host, &mut guest).await;
    hidden_without_flush(&mut host).await;
    assert!(host.debug_windows.netplay.connected && guest.debug_windows.netplay.connected);

    for (app, buttons) in [(&mut host, 0xffu8), (&mut guest, 0x81)] {
        app.game_window_focused = true;
        app.game_view_focused = true;
        app.egui_wants_keyboard = false;
        for (bit, button) in [
            crate::input::HostButton::A,
            crate::input::HostButton::B,
            crate::input::HostButton::Select,
            crate::input::HostButton::Start,
            crate::input::HostButton::Up,
            crate::input::HostButton::Down,
            crate::input::HostButton::Left,
            crate::input::HostButton::Right,
        ]
        .into_iter()
        .enumerate()
        {
            app.host_input
                .set_keyboard(button, buttons & (1 << bit) != 0);
            app.host_input.set_keyboard_p2(button, true);
        }
    }

    for _ in 0..6 {
        let target = host.netplay.confirmed + 1;
        for app in [&mut host, &mut guest] {
            app.netplay.next_frame = None;
            app.pump_netplay();
            hold(app);
        }
        let started = Instant::now();
        while host.netplay.confirmed < target
            || guest.netplay.confirmed < target
            || host.netplay.in_flight
            || guest.netplay.in_flight
        {
            for app in [&mut host, &mut guest] {
                assert_eq!(drain(app), 0);
                app.pump_netplay();
                hold(app);
                assert!(
                    app.netplay.running(),
                    "{}",
                    app.debug_windows.netplay.status
                );
            }
            assert!(started.elapsed() < Duration::from_secs(3));
            visibility_test_yield().await;
        }
    }
    for app in [&mut host, &mut guest] {
        app.request_netplay_stop();
    }
    let started = Instant::now();
    while host.netplay.fenced() || guest.netplay.fenced() {
        drain(&mut host);
        drain(&mut guest);
        assert!(started.elapsed() < Duration::from_secs(3));
        visibility_test_yield().await;
    }
    assert!(host.emu_thread.is_some() && guest.emu_thread.is_some());
    for app in [&mut host, &mut guest] {
        app.stop_game();
    }
    let started = Instant::now();
    while !host.wasm_retired_threads.is_empty() || !guest.wasm_retired_threads.is_empty() {
        host.poll_retired_wasm_threads();
        guest.poll_retired_wasm_threads();
        assert!(started.elapsed() < Duration::from_secs(5));
        visibility_test_yield().await;
    }
    js_sys::Reflect::set(&global, &name, &previous).unwrap();
}
