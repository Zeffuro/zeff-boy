use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

use super::ScanLimits;
use super::media::{ScanInput, ScanManifest};

struct PendingScan {
    source: Arc<ScanInput>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<ScanManifest>,
}

#[derive(Default)]
pub(crate) struct DiscoverySession {
    pub(crate) source: Option<Arc<ScanInput>>,
    pub(crate) manifest: Option<ScanManifest>,
    pub(crate) error: Option<String>,
    pub(crate) cancelled: bool,
    pub(crate) limits: ScanLimits,
    pending: Option<PendingScan>,
    queued: Option<ScanLimits>,
}

impl DiscoverySession {
    pub(crate) fn bind_source(&mut self, source: Option<Arc<ScanInput>>) -> bool {
        let unchanged = match (&self.source, &source) {
            (Some(current), Some(next)) => Arc::ptr_eq(current, next),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return false;
        }
        self.cancel();
        self.source = source;
        self.manifest = None;
        self.error = None;
        self.cancelled = false;
        true
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn is_queued(&self) -> bool {
        self.queued.is_some()
    }

    pub(crate) fn can_start(&self) -> bool {
        self.source.as_ref().is_some_and(|source| {
            self.queued.is_none()
                && self.pending.as_ref().is_none_or(|pending| {
                    !Arc::ptr_eq(&pending.source, source) || pending.cancel.load(Ordering::Relaxed)
                })
        })
    }

    pub(crate) fn start(&mut self) {
        self.start_with_limits(self.limits);
    }

    fn start_with_limits(&mut self, limits: ScanLimits) {
        let Some(source) = self.source.clone() else {
            return;
        };
        self.manifest = None;
        self.error = None;
        self.cancelled = false;
        if let Some(pending) = &self.pending {
            // Retain one job and one replacement request, even during rapid ROM switches.
            pending.cancel.store(true, Ordering::Relaxed);
            self.queued = Some(limits);
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_source = Arc::clone(&source);
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("zeff-audio-discovery".to_owned())
            .spawn(move || {
                let manifest = worker_source.analyze(limits, &worker_cancel);
                let _ = sender.send(manifest);
            }) {
            Ok(_) => {
                self.pending = Some(PendingScan {
                    source,
                    cancel,
                    receiver,
                });
            }
            Err(error) => self.error = Some(format!("Couldn't start audio discovery: {error}")),
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.queued = None;
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
            self.cancelled = true;
        }
    }

    pub(crate) fn poll(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Err(TryRecvError::Empty) => return,
            result => result,
        };
        if !pending.cancel.load(Ordering::Relaxed)
            && self
                .source
                .as_ref()
                .is_some_and(|source| Arc::ptr_eq(source, &pending.source))
        {
            match result {
                Ok(manifest) => self.manifest = Some(manifest),
                Err(_) => self.error = Some("Audio discovery worker stopped unexpectedly".into()),
            }
        }
        self.pending = None;
        if let Some(limits) = self.queued.take() {
            self.start_with_limits(limits);
        }
    }
}

impl Drop for DiscoverySession {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
