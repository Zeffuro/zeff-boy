use super::*;
use wasm_bindgen_test::wasm_bindgen_test;

fn paste(canvas: &HtmlCanvasElement, text: &str) -> ClipboardEvent {
    let data = web_sys::DataTransfer::new().unwrap();
    data.set_data("text/plain", text).unwrap();
    let init = web_sys::ClipboardEventInit::new();
    init.set_clipboard_data(Some(&data));
    init.set_cancelable(true);
    let event = ClipboardEvent::new_with_event_init_dict("paste", &init).unwrap();
    canvas.dispatch_event(&event).unwrap();
    event
}

#[wasm_bindgen_test]
fn browser_netplay_clipboard_dom_paste_reaches_text_edit_once_and_detaches() {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .unchecked_into();
    let context = egui::Context::default();
    let bridge = BrowserClipboard::attach(canvas.clone(), context.clone(), Weak::new()).unwrap();
    assert!(!paste(&canvas, "outside text editing").default_prevented());
    assert!(bridge.input(egui::RawInput::default()).events.is_empty());
    let mut text = String::new();
    let id = egui::Id::new("invitation");
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        ui.add(egui::TextEdit::singleline(&mut text).id(id))
            .request_focus();
    });
    let invitation = "zeff-netplay:1/room/session/delay/server";
    assert!(paste(&canvas, invitation).default_prevented());
    let mut input = egui::RawInput::default();
    input
        .events
        .push(egui::Event::Paste("stale fallback".into()));
    let _ = context.run_ui(bridge.input(input), |ui| {
        ui.add(egui::TextEdit::singleline(&mut text).id(id));
    });
    assert_eq!(text, invitation);
    assert!(bridge.input(egui::RawInput::default()).events.is_empty());
    for _ in 0..MAX_PASTES + 1 {
        paste(&canvas, "bounded");
    }
    assert_eq!(
        bridge.input(egui::RawInput::default()).events.len(),
        MAX_PASTES
    );
    paste(&canvas, &"x".repeat(MAX_TEXT + 1));
    assert!(bridge.input(egui::RawInput::default()).events.is_empty());
    drop(bridge);
    assert!(!paste(&canvas, "detached").default_prevented());
}
