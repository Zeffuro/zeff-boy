use std::sync::atomic::{AtomicU64, Ordering};

use zeff_emu_common::debug::sequence_is_after;

static NEXT_STREAM: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TraceStreamId(u64);

impl TraceStreamId {
    pub(crate) fn allocate() -> Self {
        Self::allocate_from(&NEXT_STREAM)
    }

    fn allocate_from(counter: &AtomicU64) -> Self {
        Self(
            counter
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_add(1)
                })
                .unwrap_or(u64::MAX),
        )
    }

    pub(crate) fn is_live(self) -> bool {
        // The terminal identity fences old batches without reusing an active id.
        self.0 != u64::MAX
    }

    #[cfg(test)]
    pub(crate) fn exhausted_for_test() -> Self {
        Self::allocate_from(&AtomicU64::new(u64::MAX))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TraceTarget {
    #[default]
    Primary,
    Sound,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TraceCursor {
    pub(crate) target: TraceTarget,
    pub(crate) stream: Option<TraceStreamId>,
    pub(crate) after_sequence: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TraceUpdate {
    Same,
    Replace,
    Ignore,
}

impl TraceCursor {
    pub(crate) fn update(self, stream: Option<TraceStreamId>, newest: Option<u64>) -> TraceUpdate {
        if let (Some(current), Some(incoming)) = (self.stream, stream)
            && incoming < current
        {
            return TraceUpdate::Ignore;
        }
        if self.stream != stream {
            return TraceUpdate::Replace;
        }
        if let Some(last) = self.after_sequence {
            match newest {
                Some(newest) if newest != last && !sequence_is_after(newest, last) => {
                    if stream.is_some() || newest >= last {
                        return TraceUpdate::Ignore;
                    }
                    return TraceUpdate::Replace;
                }
                None if stream.is_some() => return TraceUpdate::Ignore,
                _ => {}
            }
        }
        TraceUpdate::Same
    }
}

#[cfg(test)]
mod tests;
