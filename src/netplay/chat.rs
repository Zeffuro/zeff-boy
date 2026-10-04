use crate::platform::Instant;
use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{Result, ensure};
pub(crate) use zeff_netplay::wire::CHAT_MAX_BYTES as MAX_BYTES;

const HISTORY_LIMIT: usize = 64;
const BURST: usize = 4;
const REFILL: Duration = Duration::from_millis(500);

pub(crate) struct ChatMessage {
    pub(crate) local: bool,
    pub(crate) text: String,
}

#[derive(Default)]
pub(crate) struct Chat {
    messages: VecDeque<ChatMessage>,
    pub(crate) error: String,
}

impl Chat {
    pub(crate) fn messages(&self) -> impl Iterator<Item = &ChatMessage> {
        self.messages.iter()
    }

    pub(crate) fn push(&mut self, local: bool, text: String) {
        if self.messages.len() == HISTORY_LIMIT {
            self.messages.pop_front();
        }
        self.messages.push_back(ChatMessage { local, text });
        if local {
            self.error.clear();
        }
    }

    pub(crate) fn clear(&mut self) {
        self.messages.clear();
        self.error.clear();
    }
}

pub(super) struct RateLimit {
    tokens: usize,
    updated: Instant,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            tokens: BURST,
            updated: Instant::now(),
        }
    }
}

impl RateLimit {
    pub(super) fn take(&mut self, now: Instant) -> Result<()> {
        let elapsed = now.saturating_duration_since(self.updated);
        let refill = (elapsed.as_nanos() / REFILL.as_nanos()).min(BURST as u128) as usize;
        if refill != 0 {
            self.tokens = (self.tokens + refill).min(BURST);
            self.updated =
                now - Duration::from_nanos((elapsed.as_nanos() % REFILL.as_nanos()) as u64);
        }
        ensure!(self.tokens != 0, "Wait a moment before sending again.");
        self.tokens -= 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_is_bounded_and_clear_drops_the_previous_session() {
        let mut chat = Chat::default();
        for index in 0..100 {
            chat.push(index % 2 == 0, index.to_string());
        }
        assert_eq!(chat.messages().count(), HISTORY_LIMIT);
        assert_eq!(chat.messages().next().unwrap().text, "36");
        chat.clear();
        assert_eq!(chat.messages().count(), 0);
    }

    #[test]
    fn rate_limit_allows_a_short_burst_and_refills_without_unbounded_credit() {
        let mut limiter = RateLimit::default();
        let start = limiter.updated;
        for _ in 0..BURST {
            limiter.take(start).unwrap();
        }
        assert!(limiter.take(start).is_err());
        assert!(limiter.take(start + REFILL / 2).is_err());
        limiter.take(start + REFILL).unwrap();
        assert!(limiter.take(start + REFILL).is_err());
        let later = start + Duration::from_secs(20);
        for _ in 0..BURST {
            limiter.take(later).unwrap();
        }
        assert!(limiter.take(later).is_err());
    }
}
