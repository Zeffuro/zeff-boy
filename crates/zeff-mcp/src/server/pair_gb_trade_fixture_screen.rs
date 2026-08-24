use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use super::Server;
use super::pair_gb_trade_fixture::ensure_deadline;

impl Server {
    pub(super) fn wait_pair_trade_menu(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<PairScreenScores> {
        loop {
            ensure_deadline(deadline)?;
            let left = self.frame_scores(left_addr, deadline)?;
            let right = self.frame_scores(right_addr, deadline)?;
            if left.is_trade_menu() && right.is_trade_menu() {
                return Ok(PairScreenScores { left, right });
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub(super) fn wait_pair_blue_screen(
        &self,
        left_addr: &str,
        right_addr: &str,
        timeout: Duration,
        deadline: Instant,
    ) -> anyhow::Result<PairScreenScores> {
        let local_deadline = Instant::now() + timeout;
        loop {
            ensure_deadline(deadline)?;
            if Instant::now() >= local_deadline {
                bail!("blue link screen not reached");
            }
            let left = self.frame_scores(left_addr, deadline)?;
            let right = self.frame_scores(right_addr, deadline)?;
            if left.is_blue_screen() && right.is_blue_screen() {
                return Ok(PairScreenScores { left, right });
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub(super) fn wait_pair_trade_confirm_prompt(
        &self,
        left_addr: &str,
        right_addr: &str,
        timeout: Duration,
        deadline: Instant,
    ) -> anyhow::Result<PairScreenScores> {
        let local_deadline = Instant::now() + timeout;
        let mut last = PairScreenScores::default();
        loop {
            ensure_deadline(deadline)?;
            if Instant::now() >= local_deadline {
                bail!(
                    "trade confirmation prompt not reached; last scores={}",
                    serde_json::to_string(&last.to_json())?
                );
            }
            let left = self.frame_scores(left_addr, deadline)?;
            let right = self.frame_scores(right_addr, deadline)?;
            last = PairScreenScores { left, right };
            if left.is_trade_confirm_prompt() && right.is_trade_confirm_prompt() {
                return Ok(last);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub(super) fn wait_for_screen_settle(
        &self,
        addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        ensure_deadline(deadline)?;
        let _ = self.frame_scores(addr, deadline)?;
        std::thread::sleep(Duration::from_millis(250));
        Ok(())
    }

    pub(super) fn frame_scores(
        &self,
        addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<FrameScores> {
        let status = self.status(addr)?;
        let width = status
            .pointer("/framebuffer/screen_width")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .context("status missing framebuffer width")?;
        let height = status
            .pointer("/framebuffer/screen_height")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .context("status missing framebuffer height")?;
        anyhow::ensure!(width > 0 && height > 0, "framebuffer is not ready");

        let mut scores = FrameScores::default();
        for y in (0..height).step_by(8) {
            let start = y.saturating_mul(width).saturating_mul(4);
            let row =
                self.read_memory_bytes(addr, "framebuffer", start as u32, width * 4, deadline)?;
            scores.add_row(&row);
        }
        Ok(scores.finish())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PairScreenScores {
    pub(super) left: FrameScores,
    pub(super) right: FrameScores,
}

impl PairScreenScores {
    pub(super) fn to_json(self) -> Value {
        json!({
            "left": self.left.to_json(),
            "right": self.right.to_json(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FrameScores {
    dark_blue: usize,
    green: usize,
    white: usize,
    pink: usize,
    total: usize,
}

impl FrameScores {
    fn add_row(&mut self, bytes: &[u8]) {
        for pixel in bytes.as_chunks::<4>().0.iter().step_by(4) {
            let r = usize::from(pixel[0]);
            let g = usize::from(pixel[1]);
            let b = usize::from(pixel[2]);
            if b > 70 && b > r + 30 && b > g + 10 {
                self.dark_blue += 1;
            }
            if g > 160 && r < 80 && b < 120 {
                self.green += 1;
            }
            if r > 220 && g > 220 && b > 220 {
                self.white += 1;
            }
            if r > 200 && g < 170 && b > 150 {
                self.pink += 1;
            }
            self.total += 1;
        }
    }

    fn finish(self) -> Self {
        self
    }

    pub(super) fn is_blue_screen(self) -> bool {
        self.ratio(self.dark_blue) > 0.45 && self.ratio(self.pink) < 0.05
    }

    pub(super) fn is_trade_menu(self) -> bool {
        self.ratio(self.dark_blue) > 0.55
            && self.ratio(self.green) > 0.018
            && self.ratio(self.white) > 0.05
            && self.ratio(self.pink) < 0.02
    }

    pub(super) fn is_full_blue_dialog(self) -> bool {
        self.ratio(self.dark_blue) > 0.90
            && self.ratio(self.white) > 0.002
            && self.ratio(self.green) < 0.01
            && self.ratio(self.pink) < 0.01
    }

    pub(super) fn is_trade_confirm_prompt(self) -> bool {
        self.ratio(self.dark_blue) > 0.70
            && self.ratio(self.white) > 0.12
            && self.ratio(self.green) < 0.012
            && self.ratio(self.pink) < 0.01
    }

    pub(super) fn to_json(self) -> Value {
        json!({
            "dark_blue": self.ratio(self.dark_blue),
            "green": self.ratio(self.green),
            "white": self.ratio(self.white),
            "pink": self.ratio(self.pink),
            "sampled_pixels": self.total,
        })
    }

    fn ratio(self, value: usize) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            value as f64 / self.total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_menu_requires_green_border_and_not_room_pink() {
        let menu = FrameScores {
            dark_blue: 725,
            green: 58,
            white: 155,
            pink: 0,
            total: 1000,
        };
        let completed_dialog = FrameScores {
            dark_blue: 993,
            green: 0,
            white: 7,
            pink: 0,
            total: 1000,
        };
        let trade_room = FrameScores {
            dark_blue: 81,
            green: 4,
            white: 0,
            pink: 279,
            total: 1000,
        };
        let waiting_choice = FrameScores {
            dark_blue: 821,
            green: 17,
            white: 143,
            pink: 0,
            total: 1000,
        };

        assert!(menu.is_trade_menu());
        assert!(!menu.is_trade_confirm_prompt());
        assert!(!waiting_choice.is_trade_menu());
        assert!(!completed_dialog.is_trade_menu());
        assert!(!trade_room.is_blue_screen());
    }

    #[test]
    fn trade_confirm_prompt_has_less_green_border_than_party_menu() {
        let prompt = FrameScores {
            dark_blue: 806,
            green: 8,
            white: 167,
            pink: 0,
            total: 1000,
        };
        let waiting_menu = FrameScores {
            dark_blue: 821,
            green: 17,
            white: 143,
            pink: 0,
            total: 1000,
        };

        assert!(prompt.is_trade_confirm_prompt());
        assert!(!waiting_menu.is_trade_confirm_prompt());
    }
}
