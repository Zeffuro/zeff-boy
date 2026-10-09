use std::{cell::Cell, rc::Rc};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{AudioBufferSourceNode, AudioScheduledSourceNode};

const MAX_SOURCES: usize = 64;

struct ScheduledSource {
    node: AudioBufferSourceNode,
    ended: Rc<Cell<bool>>,
    _listener: Closure<dyn FnMut()>,
}

impl Drop for ScheduledSource {
    fn drop(&mut self) {
        let scheduled: &AudioScheduledSourceNode = self.node.as_ref();
        scheduled.set_onended(None);
        let _ = self.node.disconnect();
    }
}

#[derive(Default)]
pub(super) struct ScheduledSources {
    queued: Vec<ScheduledSource>,
}

impl ScheduledSources {
    pub(super) fn start(&mut self, node: AudioBufferSourceNode, when: f64) -> Result<(), JsValue> {
        self.prune();
        if self.is_full() {
            let _ = node.disconnect();
            return Err(JsValue::from_str("audio source queue is full"));
        }
        let ended = Rc::new(Cell::new(false));
        let completed = Rc::clone(&ended);
        let listener = Closure::wrap(Box::new(move || completed.set(true)) as Box<dyn FnMut()>);
        let scheduled: &AudioScheduledSourceNode = node.as_ref();
        scheduled.set_onended(Some(listener.as_ref().unchecked_ref()));
        let source = ScheduledSource {
            node,
            ended,
            _listener: listener,
        };
        source.node.start_with_when(when)?;
        self.queued.push(source);
        Ok(())
    }

    pub(super) fn prune(&mut self) {
        // A start time in the past does not establish when a buffer actually finishes.
        self.queued.retain(|source| !source.ended.get());
    }

    pub(super) fn is_full(&self) -> bool {
        // Bound ownership even while the main thread delays ended callbacks.
        self.queued.len() >= MAX_SOURCES
    }

    pub(super) fn cancel(&mut self) {
        for source in self.queued.drain(..) {
            let scheduled: &AudioScheduledSourceNode = source.node.as_ref();
            let _ = scheduled.stop();
        }
    }

    #[cfg(all(test, feature = "wasm-browser-tests"))]
    pub(super) fn len(&self) -> usize {
        self.queued.len()
    }
}

impl Drop for ScheduledSources {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
mod tests;
