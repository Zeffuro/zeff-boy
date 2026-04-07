use std::path::{Path, PathBuf};

pub(crate) struct FileDialog {
    accept: String,
}

impl FileDialog {
    pub(crate) fn new() -> Self {
        Self {
            accept: String::new(),
        }
    }

    pub(crate) fn set_title(self, _title: &str) -> Self {
        self
    }

    pub(crate) fn add_filter(mut self, _name: &str, extensions: &[&str]) -> Self {
        for ext in extensions {
            if !self.accept.is_empty() {
                self.accept.push(',');
            }
            self.accept.push('.');
            self.accept.push_str(ext);
        }
        self
    }

    pub(crate) fn set_file_name(self, _name: &str) -> Self {
        self
    }

    pub(crate) fn set_directory(self, _dir: impl Into<PathBuf>) -> Self {
        self
    }

    pub(crate) fn pick_file(self) -> Option<PathBuf> {
        None
    }

    pub(crate) fn pick_file_web(
        self,
        slot: std::rc::Rc<std::cell::RefCell<Option<(String, Vec<u8>)>>>,
    ) {
        trigger_file_picker(&self.accept, slot);
    }

    pub(crate) fn save_file(self) -> Option<PathBuf> {
        None
    }
}

fn trigger_file_picker(
    accept: &str,
    slot: std::rc::Rc<std::cell::RefCell<Option<(String, Vec<u8>)>>>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    let window = web_sys::window().expect("browser window must exist");
    let document = window.document().expect("document must exist");
    let input: web_sys::HtmlInputElement = document
        .create_element("input")
        .expect("failed to create input element")
        .dyn_into()
        .expect("element must be HtmlInputElement");
    input.set_type("file");
    if !accept.is_empty() {
        input.set_attribute("accept", accept).ok();
    }

    let input_clone = input.clone();
    let onchange = Closure::once(move |_: web_sys::Event| {
        let Some(files) = input_clone.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };
        read_file_into_slot(file, slot);
    });
    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

pub(crate) fn setup_drop_handler(
    target: &web_sys::EventTarget,
    slot: std::rc::Rc<std::cell::RefCell<Option<(String, Vec<u8>)>>>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    let dragover = Closure::wrap(Box::new(move |event: web_sys::DragEvent| {
        event.prevent_default();
    }) as Box<dyn FnMut(_)>);
    target
        .add_event_listener_with_callback("dragover", dragover.as_ref().unchecked_ref())
        .ok();
    dragover.forget();

    let drop_handler = Closure::wrap(Box::new(move |event: web_sys::DragEvent| {
        event.prevent_default();
        let Some(dt) = event.data_transfer() else {
            return;
        };
        let Some(files) = dt.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };
        read_file_into_slot(file, slot.clone());
    }) as Box<dyn FnMut(_)>);
    target
        .add_event_listener_with_callback("drop", drop_handler.as_ref().unchecked_ref())
        .ok();
    drop_handler.forget();
}

fn read_file_into_slot(
    file: web_sys::File,
    slot: std::rc::Rc<std::cell::RefCell<Option<(String, Vec<u8>)>>>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    let name = file.name();
    let reader = match web_sys::FileReader::new() {
        Ok(r) => r,
        Err(e) => {
            log::error!("FileReader creation failed: {e:?}");
            return;
        }
    };
    let reader_clone = reader.clone();
    let onload = Closure::once(move |_: web_sys::Event| {
        if let Ok(result) = reader_clone.result() {
            let array = js_sys::Uint8Array::new(&result);
            let data = array.to_vec();
            *slot.borrow_mut() = Some((name, data));
        }
    });
    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    if let Err(e) = reader.read_as_array_buffer(&file) {
        log::error!("FileReader read failed: {e:?}");
    }
}

pub(crate) fn screenshots_dir() -> PathBuf {
    PathBuf::from("screenshots")
}

pub(crate) fn timestamp_string() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}

pub(crate) fn format_file_modified_time(_meta: &std::fs::Metadata) -> Option<String> {
    None
}

pub(crate) fn open_url(url: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url(url);
    }
}

pub(crate) fn init_logging() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);
}

pub(crate) fn save_dir(system_subdir: &str) -> PathBuf {
    PathBuf::from(system_subdir)
}

pub(crate) fn settings_dir() -> PathBuf {
    PathBuf::from(".")
}

pub(crate) fn load_settings_json() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item("zeff-boy-settings").ok()?
}

pub(crate) fn save_settings_json(json: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item("zeff-boy-settings", json);
    }
}

pub(crate) fn download_file(filename: &str, bytes: &[u8]) {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        log::error!("download_file: no window");
        return;
    };
    let Some(document) = window.document() else {
        log::error!("download_file: no document");
        return;
    };

    let uint8 = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&uint8);

    let mut opts = web_sys::BlobPropertyBag::new();
    opts.type_("application/octet-stream");

    let blob = match web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
        Ok(b) => b,
        Err(e) => {
            log::error!("download_file: failed to create Blob: {e:?}");
            return;
        }
    };

    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(e) => {
            log::error!("download_file: failed to create object URL: {e:?}");
            return;
        }
    };

    let Ok(anchor) = document.create_element("a") else {
        log::error!("download_file: failed to create anchor");
        return;
    };
    let anchor: web_sys::HtmlAnchorElement = anchor.unchecked_into();
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.style().set_property("display", "none").ok();
    if let Some(body) = document.body() {
        let _ = body.append_child(&anchor);
        anchor.click();
        let _ = body.remove_child(&anchor);
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}
