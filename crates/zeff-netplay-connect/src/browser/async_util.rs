use super::{Callback, error};
use futures_util::future::{Either, select};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

#[derive(Clone, Default)]
pub(super) struct Cancellation(Rc<CancelState>);

#[derive(Default)]
struct CancelState {
    cancelled: Cell<bool>,
    next: Cell<u64>,
    waiters: RefCell<BTreeMap<u64, Waker>>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancelled.set(true);
        let waiters = std::mem::take(&mut *self.0.waiters.borrow_mut());
        for waiter in waiters.into_values() {
            waiter.wake();
        }
    }

    pub fn cancelled(&self) -> bool {
        self.0.cancelled.get()
    }

    pub async fn wait<T>(
        &self,
        future: impl Future<Output = Result<T, JsValue>>,
    ) -> Result<T, JsValue> {
        if self.cancelled() {
            return Err(error("connection cancelled"));
        }
        let pending = CancelWait {
            signal: self.clone(),
            id: None,
        };
        futures_util::pin_mut!(future, pending);
        match select(future, pending).await {
            Either::Left((value, _)) if !self.cancelled() => value,
            _ => Err(error("connection cancelled")),
        }
    }
}

struct CancelWait {
    signal: Cancellation,
    id: Option<u64>,
}
impl Future for CancelWait {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.signal.cancelled() {
            return Poll::Ready(());
        }
        let id = match self.id {
            Some(id) => id,
            None => {
                if self.signal.0.waiters.borrow().len() >= 16 {
                    return Poll::Ready(());
                }
                let id = self.signal.0.next.get();
                self.signal.0.next.set(id.wrapping_add(1));
                self.id = Some(id);
                id
            }
        };
        self.signal
            .0
            .waiters
            .borrow_mut()
            .insert(id, cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for CancelWait {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.signal.0.waiters.borrow_mut().remove(&id);
        }
    }
}

pub(super) struct Delay {
    id: i32,
    window: web_sys::Window,
    state: Rc<(Cell<bool>, RefCell<Option<Waker>>)>,
    _callback: Callback,
}
impl Delay {
    pub fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| error("browser window unavailable"))?;
        let state = Rc::new((Cell::new(false), RefCell::new(None::<Waker>)));
        let weak = Rc::downgrade(&state);
        let callback = Closure::wrap(Box::new(move |_: JsValue| {
            if let Some(state) = weak.upgrade() {
                state.0.set(true);
                let wake = state.1.borrow_mut().take();
                if let Some(wake) = wake {
                    wake.wake();
                }
            }
        }) as Box<dyn FnMut(JsValue)>);
        let id = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            10,
        )?;
        Ok(Self {
            id,
            window,
            state,
            _callback: callback,
        })
    }
}
impl Future for Delay {
    type Output = Result<(), JsValue>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.state.0.get() {
            Poll::Ready(Ok(()))
        } else {
            *self.state.1.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for Delay {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.id);
    }
}

pub(super) fn now() -> Result<f64, JsValue> {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|clock| clock.now())
        .ok_or_else(|| error("browser clock unavailable"))
}
