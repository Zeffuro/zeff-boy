use std::time::{Duration, Instant};

use anyhow::bail;
use serde_json::json;

use super::Server;
use super::pair_gb_trade_fixture::{
    GEN2_PARTY_COUNT_ADDR, GEN2_PARTY_MON_STRUCT_LEN, GEN2_PARTY_MON_STRUCTS_ADDR,
    GEN2_PARTY_SPECIES_ADDR, GEN2_POSITION_ADDR, GbTradePartySlot, GbTradePartySnapshot,
    GbTradePosition, TRADE_ROOM_GROUP, TRADE_ROOM_MAP, ensure_deadline, ensure_party_index,
    ensure_trade_room, frame_wait_ms, memory_response_bytes,
};
use super::pair_gb_trade_fixture_screen::PairScreenScores;

const TRADE_CONSOLE_Y: u8 = 4;
const LEFT_CONSOLE_STAND_X: u8 = 3;
const RIGHT_CONSOLE_STAND_X: u8 = 6;
const LEFT_CONSOLE_ROUTE: &[(u8, u8)] = &[(2, 5), (2, 4), (LEFT_CONSOLE_STAND_X, TRADE_CONSOLE_Y)];
const RIGHT_CONSOLE_ROUTE: &[(u8, u8)] =
    &[(7, 5), (7, 4), (RIGHT_CONSOLE_STAND_X, TRADE_CONSOLE_Y)];

impl Server {
    pub(super) fn prepare_gb_trade_fixture_room_entry(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        ensure_deadline(deadline)?;
        self.tap_both(left_addr, right_addr, "up", 8)?;
        self.tap_both(left_addr, right_addr, "a", 8)?;
        std::thread::sleep(Duration::from_millis(1_200));
        self.tap_both(left_addr, right_addr, "a", 8)?;
        std::thread::sleep(Duration::from_millis(1_200));
        Ok(())
    }

    pub(super) fn finish_gb_trade_fixture_room_entry(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        for _ in 0..120 {
            ensure_deadline(deadline)?;
            if self.pair_is_at_position(left_addr, right_addr, TRADE_ROOM_GROUP, TRADE_ROOM_MAP) {
                return Ok(());
            }
            self.tap_both(left_addr, right_addr, "a", 8)?;
            std::thread::sleep(Duration::from_millis(900));
        }
        bail!("did not enter the GB trade fixture room")
    }

    pub(super) fn route_to_gb_trade_fixture_consoles(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        ensure_trade_room(self.read_gb_trade_position(left_addr, deadline)?)?;
        ensure_trade_room(self.read_gb_trade_position(right_addr, deadline)?)?;
        self.wait_for_screen_settle(left_addr, deadline)?;
        self.wait_for_screen_settle(right_addr, deadline)?;
        std::thread::sleep(Duration::from_millis(2_000));
        for &(x, y) in LEFT_CONSOLE_ROUTE {
            self.walk_gb_trade_fixture_to_coord(left_addr, x, y, deadline)?;
        }
        for &(x, y) in RIGHT_CONSOLE_ROUTE {
            self.walk_gb_trade_fixture_to_coord(right_addr, x, y, deadline)?;
        }
        std::thread::sleep(Duration::from_millis(500));
        self.ensure_gb_trade_fixture_console_positions(left_addr, right_addr, deadline)
    }

    pub(super) fn trigger_trade_consoles(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        for _ in 0..12 {
            ensure_deadline(deadline)?;
            self.ensure_gb_trade_fixture_console_positions(left_addr, right_addr, deadline)?;
            self.tap_button(left_addr, "right", 4)?;
            self.tap_button(right_addr, "left", 4)?;
            self.tap_both(left_addr, right_addr, "a", 16)?;
            let scores =
                self.wait_pair_blue_screen(left_addr, right_addr, Duration::from_secs(6), deadline);
            if scores.is_ok() {
                return Ok(());
            }
        }
        bail!("trade consoles did not enter the link wait/menu screen")
    }

    fn ensure_gb_trade_fixture_console_positions(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let left = self.read_gb_trade_position(left_addr, deadline)?;
        let right = self.read_gb_trade_position(right_addr, deadline)?;
        ensure_trade_room(left)?;
        ensure_trade_room(right)?;
        if left.y == TRADE_CONSOLE_Y
            && left.x == LEFT_CONSOLE_STAND_X
            && right.y == TRADE_CONSOLE_Y
            && right.x == RIGHT_CONSOLE_STAND_X
        {
            Ok(())
        } else {
            bail!(
                "expected GB trade fixture console positions left=({},{}), right=({},{}); got left=({},{}), right=({},{})",
                LEFT_CONSOLE_STAND_X,
                TRADE_CONSOLE_Y,
                RIGHT_CONSOLE_STAND_X,
                TRADE_CONSOLE_Y,
                left.x,
                left.y,
                right.x,
                right.y
            )
        }
    }

    fn walk_gb_trade_fixture_to_coord(
        &self,
        addr: &str,
        target_x: u8,
        target_y: u8,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        for _ in 0..48 {
            ensure_deadline(deadline)?;
            let position = self.read_gb_trade_position(addr, deadline)?;
            ensure_trade_room(position)?;
            if position.x == target_x && position.y == target_y {
                return Ok(());
            }

            let button = movement_toward(position, target_x, target_y)
                .expect("non-target position requires a movement direction");
            self.tap_button(addr, button, 2)?;
            self.wait_for_screen_settle(addr, deadline)?;
        }

        let position = self.read_gb_trade_position(addr, deadline)?;
        bail!(
            "could not walk GB trade fixture actor to ({target_x},{target_y}); got ({},{})",
            position.x,
            position.y
        )
    }

    pub(super) fn select_trade_mon(
        &self,
        addr: &str,
        party_index: u8,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        ensure_party_index(party_index)?;
        for _ in 0..party_index {
            self.tap_button(addr, "down", 8)?;
            self.wait_for_screen_settle(addr, deadline)?;
        }
        self.tap_button(addr, "a", 16)?;
        self.wait_for_screen_settle(addr, deadline)?;
        std::thread::sleep(Duration::from_millis(500));
        self.tap_button(addr, "right", 16)?;
        self.wait_for_screen_settle(addr, deadline)?;
        std::thread::sleep(Duration::from_millis(350));
        self.tap_button(addr, "a", 16)?;
        std::thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    pub(super) fn confirm_trade(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        self.wait_pair_trade_confirm_prompt(
            left_addr,
            right_addr,
            Duration::from_secs(16),
            deadline,
        )?;
        for _ in 0..4 {
            self.tap_both(left_addr, right_addr, "a", 12)?;
            std::thread::sleep(Duration::from_millis(800));
            let left = self.frame_scores(left_addr, deadline)?;
            let right = self.frame_scores(right_addr, deadline)?;
            if left.is_full_blue_dialog() && right.is_full_blue_dialog() {
                return Ok(());
            }
        }
        Ok(())
    }

    pub(super) fn wait_for_trade_completion(
        &self,
        left_addr: &str,
        right_addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<PairScreenScores> {
        let mut last_non_menu = None;
        for _ in 0..80 {
            ensure_deadline(deadline)?;
            let scores = PairScreenScores {
                left: self.frame_scores(left_addr, deadline)?,
                right: self.frame_scores(right_addr, deadline)?,
            };
            if scores.left.is_trade_menu() && scores.right.is_trade_menu() {
                return Ok(last_non_menu.unwrap_or(scores));
            }
            last_non_menu = Some(scores);
            self.tap_both(left_addr, right_addr, "a", 12)?;
            std::thread::sleep(Duration::from_millis(1_500));
        }
        bail!("trade completion screen was not dismissed back to the trade menu")
    }

    pub(super) fn wait_for_position(
        &self,
        addr: &str,
        group: u8,
        map: u8,
        deadline: Instant,
    ) -> anyhow::Result<GbTradePosition> {
        loop {
            ensure_deadline(deadline)?;
            let position = self.read_gb_trade_position(addr, deadline)?;
            if position.group == group && position.map == map {
                return Ok(position);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub(super) fn pair_is_at_position(
        &self,
        left_addr: &str,
        right_addr: &str,
        group: u8,
        map: u8,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let left = self.read_gb_trade_position(left_addr, deadline).ok();
        let right = self.read_gb_trade_position(right_addr, deadline).ok();
        left.is_some_and(|position| position.group == group && position.map == map)
            && right.is_some_and(|position| position.group == group && position.map == map)
    }

    pub(super) fn read_gb_trade_position(
        &self,
        addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<GbTradePosition> {
        let bytes = self.read_memory_bytes(addr, "cpu", GEN2_POSITION_ADDR, 4, deadline)?;
        let [group, map, y, x] = bytes.as_slice() else {
            bail!(
                "GB trade fixture position read returned {} bytes",
                bytes.len()
            );
        };
        Ok(GbTradePosition::new(*group, *map, *y, *x))
    }

    pub(super) fn read_gb_trade_party_snapshot(
        &self,
        addr: &str,
        deadline: Instant,
    ) -> anyhow::Result<GbTradePartySnapshot> {
        let party_count =
            self.read_memory_bytes(addr, "cpu", GEN2_PARTY_COUNT_ADDR, 1, deadline)?[0];
        anyhow::ensure!(
            party_count <= 6,
            "invalid fixture party count {party_count}"
        );

        let species = self.read_memory_bytes(
            addr,
            "cpu",
            GEN2_PARTY_SPECIES_ADDR,
            usize::from(party_count) + 1,
            deadline,
        )?;
        let structs = self.read_memory_bytes(
            addr,
            "cpu",
            GEN2_PARTY_MON_STRUCTS_ADDR,
            usize::from(party_count) * GEN2_PARTY_MON_STRUCT_LEN,
            deadline,
        )?;
        let slots = (0..party_count)
            .map(|index| {
                let start = usize::from(index) * GEN2_PARTY_MON_STRUCT_LEN;
                GbTradePartySlot::new(
                    index,
                    party_count,
                    species[usize::from(index)],
                    structs[start..start + GEN2_PARTY_MON_STRUCT_LEN].to_vec(),
                )
            })
            .collect();
        Ok(GbTradePartySnapshot::new(slots))
    }

    pub(super) fn read_memory_bytes(
        &self,
        addr: &str,
        space: &str,
        start: u32,
        length: usize,
        deadline: Instant,
    ) -> anyhow::Result<Vec<u8>> {
        if space == "cpu" {
            self.wait_for_memory_bytes(
                addr,
                space,
                freshness_probe_start(start, length),
                1,
                deadline,
            )?;
        }
        self.wait_for_memory_bytes(addr, space, start, length, deadline)
    }

    fn wait_for_memory_bytes(
        &self,
        addr: &str,
        space: &str,
        start: u32,
        length: usize,
        deadline: Instant,
    ) -> anyhow::Result<Vec<u8>> {
        loop {
            ensure_deadline(deadline)?;
            let response = self.call_live_at(
                addr,
                json!({
                    "command": "memory",
                    "space": space,
                    "start": start,
                    "length": length,
                }),
            )?;
            if response.get("ready").and_then(serde_json::Value::as_bool) == Some(true) {
                return memory_response_bytes(&response);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub(super) fn tap_both(
        &self,
        left_addr: &str,
        right_addr: &str,
        button: &str,
        frames: u64,
    ) -> anyhow::Result<()> {
        self.tap_button(left_addr, button, frames)?;
        self.tap_button(right_addr, button, frames)?;
        Ok(())
    }

    pub(super) fn tap_button(&self, addr: &str, button: &str, frames: u64) -> anyhow::Result<()> {
        self.call_live_at(
            addr,
            json!({
                "command": "tap",
                "button": button,
                "frames": frames,
            }),
        )?;
        std::thread::sleep(Duration::from_millis(frame_wait_ms(frames)));
        Ok(())
    }
}

fn movement_toward(position: GbTradePosition, target_x: u8, target_y: u8) -> Option<&'static str> {
    if position.y < target_y {
        Some("down")
    } else if position.y > target_y {
        Some("up")
    } else if position.x < target_x {
        Some("right")
    } else if position.x > target_x {
        Some("left")
    } else {
        None
    }
}

fn freshness_probe_start(start: u32, length: usize) -> u32 {
    start
        .checked_add(u32::try_from(length).unwrap_or(u32::MAX))
        .filter(|&probe| probe <= u32::from(u16::MAX))
        .unwrap_or_else(|| start.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_finishes_vertical_movement_before_horizontal_approach() {
        let position = GbTradePosition::new(TRADE_ROOM_GROUP, TRADE_ROOM_MAP, 5, 4);

        assert_eq!(movement_toward(position, 3, 4), Some("up"));
        assert_eq!(movement_toward(position, 6, 5), Some("right"));
        assert_eq!(movement_toward(position, 4, 5), None);
    }

    #[test]
    fn console_routes_use_clear_outer_lanes() {
        assert_eq!(LEFT_CONSOLE_ROUTE, &[(2, 5), (2, 4), (3, 4)]);
        assert_eq!(RIGHT_CONSOLE_ROUTE, &[(7, 5), (7, 4), (6, 4)]);
    }

    #[test]
    fn freshness_probe_uses_a_different_cpu_page_start() {
        assert_eq!(freshness_probe_start(0xDCB5, 4), 0xDCB9);
        assert_eq!(freshness_probe_start(0xFFFF, 1), 0xFFFE);
    }
}
