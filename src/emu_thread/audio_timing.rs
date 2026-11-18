use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct Record {
    pub(crate) at_ns: u64,
    pub(crate) request: u64,
    pub(crate) stage: &'static str,
    pub(crate) field: u64,
    pub(crate) frames: u64,
    pub(crate) scalars: u64,
    pub(crate) aux: u64,
}

pub(crate) struct Shared {
    epoch: Instant,
    next: AtomicU64,
    lost: AtomicUsize,
    tx: Sender<Record>,
}

#[derive(Clone)]
struct Context {
    shared: Arc<Shared>,
    request: u64,
    fields: usize,
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

pub(crate) struct Session {
    shared: Arc<Shared>,
    rx: Receiver<Record>,
}

impl Session {
    pub(crate) fn start(epoch: Instant) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(524_288);
        let shared = Arc::new(Shared {
            epoch,
            next: AtomicU64::new(1),
            lost: AtomicUsize::new(0),
            tx,
        });
        install(Some(shared.clone()));
        Self { shared, rx }
    }

    pub(crate) fn take_records(&self) -> Vec<Record> {
        self.rx.try_iter().collect()
    }

    pub(crate) fn finish(self) -> Vec<Record> {
        install(None);
        assert_eq!(self.shared.lost.load(Ordering::Relaxed), 0);
        let mut records: Vec<_> = self.rx.try_iter().collect();
        records.sort_by_key(|record| record.at_ns);
        records
    }
}

pub(crate) fn current() -> Option<Arc<Shared>> {
    CONTEXT.with(|slot| slot.borrow().as_ref().map(|c| c.shared.clone()))
}

pub(crate) fn install(shared: Option<Arc<Shared>>) {
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = shared.map(|shared| Context {
            shared,
            request: 0,
            fields: 0,
        })
    });
}

pub(crate) fn request(id: u64) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow_mut().as_mut() {
            context.request = id;
            context.fields = 0;
        }
    });
}

pub(crate) fn dispatch(shared: &Shared, frames: usize, pacing: (u64, u64)) -> u64 {
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    emit(
        shared,
        id,
        "dispatch_begin",
        [pacing.1, frames as u64, 0, pacing.0],
    );
    id
}

pub(crate) fn dispatched(shared: &Shared, id: u64, sent: bool) {
    emit(shared, id, "dispatch_end", [0, 0, 0, u64::from(sent)]);
}

pub(crate) fn step_end(index: usize, frames: usize) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot
            .borrow()
            .as_ref()
            .filter(|c| c.request != 0 && c.fields == index)
        {
            emit(
                &context.shared,
                context.request,
                "field_step_end",
                [index as u64, frames as u64, 0, 0],
            );
        }
    });
}

pub(crate) fn field(index: usize, frames: usize, scalars: usize) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow_mut().as_mut().filter(|c| c.request != 0) {
            emit(
                &context.shared,
                context.request,
                "field_audio_ready",
                [index as u64, frames as u64, scalars as u64, 0],
            );
            context.fields += 1;
        }
    });
}

pub(crate) fn single_field(frames: usize, scalars: usize) {
    let needed = CONTEXT.with(|slot| slot.borrow().as_ref().is_some_and(|c| c.fields == 0));
    if needed && frames == 1 {
        field(0, frames, scalars);
    }
}

pub(crate) fn record(
    id: u64,
    stage: &'static str,
    field: u64,
    frames: u64,
    scalars: u64,
    aux: u64,
) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow().as_ref() {
            emit(&context.shared, id, stage, [field, frames, scalars, aux]);
        }
    });
}

pub(crate) fn delivery(ids: &[u64], stage: &'static str, frames: usize, scalars: usize, aux: u64) {
    for &id in ids {
        record(id, stage, 0, frames as u64, scalars as u64, aux);
    }
}

fn emit(
    shared: &Shared,
    request: u64,
    stage: &'static str,
    [field, frames, scalars, aux]: [u64; 4],
) {
    if shared
        .tx
        .try_send(Record {
            at_ns: shared.epoch.elapsed().as_nanos() as u64,
            request,
            stage,
            field,
            frames,
            scalars,
            aux,
        })
        .is_err()
    {
        shared.lost.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn worker_dequeue(command: &super::EmuCommand) {
    if let super::EmuCommand::StepFrames(input) = command {
        request(input.audio_timing_request);
        record(
            input.audio_timing_request,
            "worker_dequeue",
            0,
            input.frames as u64,
            0,
            0,
        );
    }
}

pub(crate) fn result(id: u64, stage: &'static str, result: &super::FrameResult) {
    record(
        id,
        stage,
        0,
        result.advanced_frames as u64,
        result.audio_samples.len() as u64,
        0,
    );
}

pub(crate) fn ready(id: u64, result: &mut super::FrameResult) {
    if id != 0 {
        result.audio_timing_requests.push(id);
        self::result(id, "result_ready", result);
    }
}
