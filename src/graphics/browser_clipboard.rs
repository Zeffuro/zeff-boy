use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use anyhow::{Context, Result};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{ClipboardEvent, Event, HtmlCanvasElement, KeyboardEvent};
use winit::{platform::web::WindowExtWebSys, window::Window};

const MAX_TEXT: usize = 16 * 1024;
const MAX_PASTES: usize = 4;

struct State {
    pastes: RefCell<VecDeque<String>>,
    writing: Cell<bool>,
    previous_prevention: Cell<Option<bool>>,
    notice: Cell<Option<&'static str>>,
    context: egui::Context,
    window: Weak<Window>,
}

impl State {
    fn repaint(&self) {
        self.context.request_repaint();
        if let Some(window) = self.window.upgrade() {
            window.request_redraw();
        }
    }

    fn failure(&self, message: &'static str) {
        self.notice.set(Some(message));
        self.repaint();
    }
}

struct Listener {
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
    capture: bool,
}

pub(super) struct BrowserClipboard {
    canvas: HtmlCanvasElement,
    state: Rc<State>,
    listeners: Vec<Listener>,
}

impl BrowserClipboard {
    pub fn new(window: &Arc<Window>, context: egui::Context) -> Result<Self> {
        Self::attach(
            window.canvas().context("Browser canvas unavailable")?,
            context,
            Arc::downgrade(window),
        )
    }

    fn attach(
        canvas: HtmlCanvasElement,
        context: egui::Context,
        window: Weak<Window>,
    ) -> Result<Self> {
        let state = Rc::new(State {
            pastes: RefCell::new(VecDeque::new()),
            writing: Cell::new(false),
            previous_prevention: Cell::new(None),
            notice: Cell::new(None),
            context,
            window,
        });
        let mut bridge = Self {
            canvas,
            state,
            listeners: Vec::new(),
        };
        let weak = Rc::downgrade(&bridge.state);
        bridge.listen("paste", true, move |event| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if !state.context.egui_wants_keyboard_input() {
                return;
            }
            let event: ClipboardEvent = event.unchecked_into();
            let text = event
                .clipboard_data()
                .and_then(|data| data.get_data("text/plain").ok());
            let Some(text) = text.filter(|text| !text.is_empty()) else {
                return;
            };
            event.prevent_default();
            if text.len() <= MAX_TEXT && state.pastes.borrow().len() < MAX_PASTES {
                state
                    .pastes
                    .borrow_mut()
                    .push_back(text.replace("\r\n", "\n"));
                state.repaint();
            } else {
                state.failure("Pasted text is too long.");
            }
        })?;
        let weak = Rc::downgrade(&bridge.state);
        bridge.listen("keydown", true, move |event| {
            let event: KeyboardEvent = event.unchecked_into();
            if !(event.ctrl_key() || event.meta_key())
                || event.alt_key()
                || !matches!(event.code().as_str(), "KeyC" | "KeyX" | "KeyV")
            {
                return;
            }
            let Some(state) = weak.upgrade() else {
                return;
            };
            if !state.context.egui_wants_keyboard_input() {
                return;
            }
            let Some(window) = state.window.upgrade() else {
                return;
            };
            let previous = window.prevent_default();
            state.previous_prevention.set(Some(previous));
            window.set_prevent_default(false);
        })?;
        let weak = Rc::downgrade(&bridge.state);
        bridge.listen("keydown", false, move |_| {
            if let Some(state) = weak.upgrade()
                && let Some(previous) = state.previous_prevention.take()
                && let Some(window) = state.window.upgrade()
            {
                window.set_prevent_default(previous);
            }
        })?;
        Ok(bridge)
    }

    fn listen(
        &mut self,
        name: &'static str,
        capture: bool,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<()> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        self.canvas
            .add_event_listener_with_callback_and_bool(
                name,
                callback.as_ref().unchecked_ref(),
                capture,
            )
            .map_err(|_| anyhow::anyhow!("Browser clipboard listener unavailable"))?;
        self.listeners.push(Listener {
            name,
            callback,
            capture,
        });
        Ok(())
    }

    pub fn input(&self, mut input: egui::RawInput) -> egui::RawInput {
        input
            .events
            .retain(|event| !matches!(event, egui::Event::Paste(_)));
        input.events.extend(
            self.state
                .pastes
                .borrow_mut()
                .drain(..)
                .map(egui::Event::Paste),
        );
        input
    }

    pub fn output(&self, output: &egui::PlatformOutput) {
        let Some(text) = output
            .commands
            .iter()
            .rev()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
        else {
            return;
        };
        if text.len() > MAX_TEXT {
            self.state.failure("Copied text is too long.");
            return;
        }
        if self.state.writing.get() {
            self.state.failure("Clipboard is busy. Copy again.");
            return;
        }
        let clipboard = web_sys::window()
            .and_then(|window| js_sys::Reflect::get(&window.navigator(), &"clipboard".into()).ok())
            .filter(JsValue::is_object);
        let Some(clipboard) = clipboard else {
            self.state
                .failure("Copy failed. Allow clipboard access and retry.");
            return;
        };
        self.state.writing.set(true);
        self.state.notice.set(None);
        let clipboard: web_sys::Clipboard = clipboard.unchecked_into();
        let promise = clipboard.write_text(text);
        let weak = Rc::downgrade(&self.state);
        wasm_bindgen_futures::spawn_local(async move {
            let result = wasm_bindgen_futures::JsFuture::from(promise).await;
            if let Some(state) = weak.upgrade() {
                state.writing.set(false);
                if result.is_err() {
                    state.failure("Copy failed. Allow clipboard access and retry.");
                }
            }
        });
    }

    pub fn show_notice(&self) {
        let Some(message) = self.state.notice.get() else {
            return;
        };
        egui::Window::new("Clipboard")
            .collapsible(false)
            .resizable(false)
            .show(&self.state.context, |ui| {
                ui.label(message);
                if ui.button("Close").clicked() {
                    self.state.notice.set(None);
                }
            });
    }
}

impl Drop for BrowserClipboard {
    fn drop(&mut self) {
        for listener in &self.listeners {
            let _ = self.canvas.remove_event_listener_with_callback_and_bool(
                listener.name,
                listener.callback.as_ref().unchecked_ref(),
                listener.capture,
            );
        }
        self.state.pastes.borrow_mut().clear();
        if let Some(previous) = self.state.previous_prevention.take()
            && let Some(window) = self.state.window.upgrade()
        {
            window.set_prevent_default(previous);
        }
    }
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
mod tests;
