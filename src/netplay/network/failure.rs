use super::*;
use std::time::Instant;

impl Network {
    pub(super) fn cancelled_reason(&self) -> String {
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            if let Some(reason) = self
                .failure
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
            {
                return reason.clone();
            }
            if self.owner_cancelled.load(Ordering::Acquire) {
                return "netplay network is cancelled".into();
            }
            if self.worker.as_ref().is_none_or(JoinHandle::is_finished)
                || Instant::now() >= deadline
            {
                return "netplay network is closed".into();
            }
            // Split receiver termination precedes the supervisor's diagnostic publication.
            thread::park_timeout(Duration::from_millis(1));
        }
    }
}
