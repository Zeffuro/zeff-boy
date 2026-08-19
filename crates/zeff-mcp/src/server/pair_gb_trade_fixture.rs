use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use super::Server;
use super::pair_gb_trade_fixture_config::GbTradeFixtureConfig;

pub(super) const GEN2_POSITION_ADDR: u32 = 0xDCB5;
pub(super) const GEN2_PARTY_COUNT_ADDR: u32 = 0xDCD7;
pub(super) const GEN2_PARTY_SPECIES_ADDR: u32 = GEN2_PARTY_COUNT_ADDR + 1;
pub(super) const GEN2_PARTY_MON_STRUCTS_ADDR: u32 = GEN2_PARTY_SPECIES_ADDR + 7;
pub(super) const GEN2_PARTY_MON_STRUCT_LEN: usize = 48;
pub(super) const GEN2_PARTY_MON_LEVEL_OFFSET: usize = 31;
const GEN2_PARTY_MON_DVS_OFFSET: usize = 21;
const GEN2_PARTY_MON_DVS_LEN: usize = 2;
pub(super) const TRADE_ROOM_GROUP: u8 = 20;
const LINK_LOBBY_MAP: u8 = 1;
pub(super) const TRADE_ROOM_MAP: u8 = 2;

impl Server {
    pub(super) fn tool_pair_gb_trade_fixture(&mut self, args: &Value) -> anyhow::Result<Value> {
        if !self.pair_is_running()? {
            bail!("no tracked Zeff Boy pair is running");
        }

        let (left_addr, right_addr) = self.pair_addrs()?;
        let config = GbTradeFixtureConfig::from_args(&self.state.repo_root, args)?;
        let deadline = Instant::now() + Duration::from_secs(config.timeout_seconds);

        config.prepare_paths(&self.state.repo_root)?;
        let outcome = self.run_gb_trade_fixture(&left_addr, &right_addr, &config, deadline);

        self.clear_gb_trade_fast_forward(&left_addr, &right_addr);
        if outcome.is_err() && config.record_replay {
            self.stop_partial_gb_trade_recording(&left_addr, &right_addr);
        }

        outcome
    }

    fn run_gb_trade_fixture(
        &self,
        left_addr: &str,
        right_addr: &str,
        config: &GbTradeFixtureConfig,
        deadline: Instant,
    ) -> anyhow::Result<Value> {
        let mut timings = GbTradeFixtureTimings::start();

        self.load_state_pair(left_addr, right_addr, &config.state_path)?;
        self.wait_for_position(left_addr, TRADE_ROOM_GROUP, LINK_LOBBY_MAP, deadline)
            .context("left/host did not load at the expected fixture lobby position")?;
        self.wait_for_position(right_addr, TRADE_ROOM_GROUP, LINK_LOBBY_MAP, deadline)
            .context("right/join did not load at the expected fixture lobby position")?;
        timings.mark("load_fixture_lobby");

        self.prepare_gb_trade_fixture_room_entry(left_addr, right_addr, deadline)?;
        timings.mark("prepare_room_entry");
        self.host_join_pair(left_addr, right_addr, &config.link_addr)?;
        timings.mark("host_join_link");
        if config.record_replay {
            self.start_replay_pair(left_addr, right_addr, config)?;
            self.wait_pair_recording_progress(left_addr, right_addr, deadline)?;
            timings.mark("start_replay_recording");
        }
        self.finish_gb_trade_fixture_room_entry(left_addr, right_addr, deadline)?;
        self.wait_for_position(left_addr, TRADE_ROOM_GROUP, TRADE_ROOM_MAP, deadline)
            .context("left/host did not enter the trade fixture room")?;
        self.wait_for_position(right_addr, TRADE_ROOM_GROUP, TRADE_ROOM_MAP, deadline)
            .context("right/join did not enter the trade fixture room")?;
        timings.mark("enter_trade_room");
        self.route_to_gb_trade_fixture_consoles(left_addr, right_addr, deadline)
            .context("route to trade fixture consoles failed")?;
        timings.mark("route_to_consoles");
        self.trigger_trade_consoles(left_addr, right_addr, deadline)?;
        timings.mark("trigger_trade_consoles");

        self.wait_pair_trade_menu(left_addr, right_addr, deadline)?;
        timings.mark("please_wait_to_trade_menu");
        let initial_left_party = self.read_gb_trade_party_snapshot(left_addr, deadline)?;
        let initial_right_party = self.read_gb_trade_party_snapshot(right_addr, deadline)?;
        std::thread::sleep(Duration::from_millis(1_000));
        self.select_trade_mon(left_addr, config.left_party_index, deadline)?;
        self.select_trade_mon(right_addr, config.right_party_index, deadline)?;
        timings.mark("select_trade_mons");
        self.confirm_trade(left_addr, right_addr, deadline)?;
        timings.mark("confirm_trade");
        if config.fast_forward {
            self.call_live_at(
                left_addr,
                json!({ "command": "fast_forward", "enabled": true }),
            )?;
            self.call_live_at(
                right_addr,
                json!({ "command": "fast_forward", "enabled": true }),
            )?;
        }

        let completion = self.wait_for_trade_completion(left_addr, right_addr, deadline)?;
        timings.mark("trade_complete_to_menu");
        let final_menu = self.wait_pair_trade_menu(left_addr, right_addr, deadline)?;
        timings.mark("final_trade_menu_confirmed");
        let party_validation = GbTradePartyValidation {
            initial_left: initial_left_party,
            initial_right: initial_right_party,
            final_left: self.read_gb_trade_party_snapshot(left_addr, deadline)?,
            final_right: self.read_gb_trade_party_snapshot(right_addr, deadline)?,
        };
        if !party_validation.valid_clean_trade() {
            bail!(
                "GB trade fixture party validation failed: expected exactly one distinct identity to cross each way"
            );
        }

        let replay = if config.record_replay {
            let replay = self.stop_replay_pair(left_addr, right_addr, config, deadline)?;
            timings.mark("stop_replay_recording");
            Some(replay)
        } else {
            None
        };

        Ok(json!({
            "completed": true,
            "left": {
                "addr": left_addr,
                "position": self.read_gb_trade_position(left_addr, deadline)?.to_json(),
                "screen": final_menu.left.to_json(),
                "status": self.call_live_at(left_addr, json!({ "command": "status" })).ok(),
            },
            "right": {
                "addr": right_addr,
                "position": self.read_gb_trade_position(right_addr, deadline)?.to_json(),
                "screen": final_menu.right.to_json(),
                "status": self.call_live_at(right_addr, json!({ "command": "status" })).ok(),
            },
            "completion_screen": completion.to_json(),
            "party_validation": party_validation.to_json(),
            "replay": replay,
            "timings": timings.to_json(),
        }))
    }
}

struct GbTradeFixtureTimings {
    start: Instant,
    last: Instant,
    phases: Vec<GbTradeFixturePhaseTiming>,
}

impl GbTradeFixtureTimings {
    fn start() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            phases: Vec::new(),
        }
    }

    fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        self.phases.push(GbTradeFixturePhaseTiming {
            name,
            elapsed_ms: duration_ms(now.duration_since(self.last)),
            total_ms: duration_ms(now.duration_since(self.start)),
        });
        self.last = now;
    }

    fn to_json(&self) -> Value {
        json!({
            "total_ms": duration_ms(self.start.elapsed()),
            "phases": self.phases.iter().map(GbTradeFixturePhaseTiming::to_json).collect::<Vec<_>>(),
        })
    }
}

struct GbTradeFixturePhaseTiming {
    name: &'static str,
    elapsed_ms: u64,
    total_ms: u64,
}

impl GbTradeFixturePhaseTiming {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "elapsed_ms": self.elapsed_ms,
            "total_ms": self.total_ms,
        })
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GbTradePartySlot {
    index: u8,
    party_count: u8,
    species_list_entry: u8,
    species: u8,
    level: u8,
    data: Vec<u8>,
}

impl GbTradePartySlot {
    pub(super) fn new(index: u8, party_count: u8, species_list_entry: u8, data: Vec<u8>) -> Self {
        let species = data.first().copied().unwrap_or(0);
        let level = data
            .get(GEN2_PARTY_MON_LEVEL_OFFSET)
            .copied()
            .unwrap_or_default();
        Self {
            index,
            party_count,
            species_list_entry,
            species,
            level,
            data,
        }
    }

    fn identity_fingerprint(&self) -> u64 {
        // Trading updates some of the received mon's mutable metadata (for
        // example its caught data).  Keep the exchange check on the stable
        // identity fields: species, held item, moves, OT ID, DVs, and level.
        let mut hash = FNV1A64_OFFSET;
        hash = fnv1a64_update(hash, self.data.get(..8).unwrap_or(&[]));
        hash = fnv1a64_update(
            hash,
            self.data
                .get(GEN2_PARTY_MON_DVS_OFFSET..GEN2_PARTY_MON_DVS_OFFSET + GEN2_PARTY_MON_DVS_LEN)
                .unwrap_or(&[]),
        );
        fnv1a64_update(
            hash,
            self.data
                .get(GEN2_PARTY_MON_LEVEL_OFFSET..=GEN2_PARTY_MON_LEVEL_OFFSET)
                .unwrap_or(&[]),
        )
    }

    fn full_fingerprint(&self) -> u64 {
        fnv1a64(&self.data)
    }

    pub(super) fn same_trade_identity_as(&self, other: &Self) -> bool {
        self.species_list_entry == other.species_list_entry
            && self.species == other.species
            && self.level == other.level
            && self.identity_fingerprint() == other.identity_fingerprint()
    }

    fn to_json(&self) -> Value {
        json!({
            "index": self.index,
            "party_count": self.party_count,
            "species_list_entry": self.species_list_entry,
            "species": self.species,
            "level": self.level,
            "identity_fingerprint": format!("{:016X}", self.identity_fingerprint()),
            "full_fingerprint": format!("{:016X}", self.full_fingerprint()),
            "struct_len": self.data.len(),
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct GbTradePartySnapshot {
    slots: Vec<GbTradePartySlot>,
}

impl GbTradePartySnapshot {
    pub(super) fn new(slots: Vec<GbTradePartySlot>) -> Self {
        Self { slots }
    }

    fn changed_indices_against(&self, initial: &Self) -> Vec<u8> {
        self.slots
            .iter()
            .zip(&initial.slots)
            .filter_map(|(final_slot, initial_slot)| {
                (!final_slot.same_trade_identity_as(initial_slot)).then_some(final_slot.index)
            })
            .collect()
    }

    fn identity_multiset(&self) -> Vec<u64> {
        let mut identities = self
            .slots
            .iter()
            .map(GbTradePartySlot::identity_fingerprint)
            .collect::<Vec<_>>();
        identities.sort_unstable();
        identities
    }

    fn to_json(&self) -> Value {
        json!({
            "slots": self.slots.iter().map(GbTradePartySlot::to_json).collect::<Vec<_>>(),
        })
    }
}

struct GbTradePartyValidation {
    initial_left: GbTradePartySnapshot,
    initial_right: GbTradePartySnapshot,
    final_left: GbTradePartySnapshot,
    final_right: GbTradePartySnapshot,
}

impl GbTradePartyValidation {
    fn valid_clean_trade(&self) -> bool {
        if self.initial_left.slots.len() != self.final_left.slots.len()
            || self.initial_right.slots.len() != self.final_right.slots.len()
        {
            return false;
        }

        self.initial_left.slots.iter().any(|left_sent| {
            self.initial_right.slots.iter().any(|right_sent| {
                left_sent.identity_fingerprint() != right_sent.identity_fingerprint()
                    && self.final_left.identity_multiset()
                        == exchanged_identity_multiset(&self.initial_left, left_sent, right_sent)
                    && self.final_right.identity_multiset()
                        == exchanged_identity_multiset(&self.initial_right, right_sent, left_sent)
            })
        })
    }

    fn to_json(&self) -> Value {
        json!({
            "valid_clean_trade": self.valid_clean_trade(),
            "left_changed_indices": self.final_left.changed_indices_against(&self.initial_left),
            "right_changed_indices": self.final_right.changed_indices_against(&self.initial_right),
            "initial_left": self.initial_left.to_json(),
            "initial_right": self.initial_right.to_json(),
            "final_left": self.final_left.to_json(),
            "final_right": self.final_right.to_json(),
        })
    }
}

fn exchanged_identity_multiset(
    initial: &GbTradePartySnapshot,
    sent: &GbTradePartySlot,
    received: &GbTradePartySlot,
) -> Vec<u64> {
    let mut identities = initial.identity_multiset();
    let sent_identity = sent.identity_fingerprint();
    let received_identity = received.identity_fingerprint();
    let Some(index) = identities
        .iter()
        .position(|identity| *identity == sent_identity)
    else {
        return Vec::new();
    };
    identities[index] = received_identity;
    identities.sort_unstable();
    identities
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_update(FNV1A64_OFFSET, bytes)
}

const FNV1A64_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fnv1a64_update(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV1A64_PRIME)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GbTradePosition {
    pub(super) group: u8,
    pub(super) map: u8,
    pub(super) y: u8,
    pub(super) x: u8,
}

impl GbTradePosition {
    pub(super) const fn new(group: u8, map: u8, y: u8, x: u8) -> Self {
        Self { group, map, y, x }
    }

    pub(super) fn to_json(self) -> Value {
        json!({
            "map_group": self.group,
            "map_number": self.map,
            "y": self.y,
            "x": self.x,
        })
    }
}

pub(super) fn ensure_party_index(index: u8) -> anyhow::Result<()> {
    if index <= 5 {
        Ok(())
    } else {
        bail!("party index must be between 0 and 5")
    }
}

pub(super) fn ensure_trade_room(position: GbTradePosition) -> anyhow::Result<()> {
    if position.group == TRADE_ROOM_GROUP && position.map == TRADE_ROOM_MAP {
        Ok(())
    } else {
        bail!(
            "expected GB trade-room map {},{}; got {},{}",
            TRADE_ROOM_GROUP,
            TRADE_ROOM_MAP,
            position.group,
            position.map
        )
    }
}

pub(super) fn memory_response_bytes(response: &Value) -> anyhow::Result<Vec<u8>> {
    let bytes = response
        .get("bytes")
        .and_then(Value::as_array)
        .context("memory response missing bytes")?;
    bytes
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .context("memory response contains a non-byte value")
        })
        .collect()
}

pub(super) fn frame_wait_ms(frames: u64) -> u64 {
    frames.saturating_mul(1000).saturating_add(59) / 60 + 60
}

pub(super) fn ensure_deadline(deadline: Instant) -> anyhow::Result<()> {
    if Instant::now() >= deadline {
        bail!("GB trade fixture automation timed out")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_fixture_position_json_uses_map_coord_order() {
        let json = GbTradePosition::new(20, 2, 4, 3).to_json();
        assert_eq!(json["map_group"], 20);
        assert_eq!(json["map_number"], 2);
        assert_eq!(json["y"], 4);
        assert_eq!(json["x"], 3);
    }

    #[test]
    fn trade_fixture_timings_report_phase_and_total_millis() {
        let mut timings = GbTradeFixtureTimings::start();
        timings.mark("phase");
        let json = timings.to_json();
        assert_eq!(json["phases"][0]["name"], "phase");
        assert!(json["phases"][0]["elapsed_ms"].as_u64().is_some());
        assert!(json["total_ms"].as_u64().is_some());
    }

    #[test]
    fn party_validation_accepts_a_single_crossed_identity_after_party_compaction() {
        let slot_a = party_slot(0, 0x15, 7, &[1, 2, 3]);
        let slot_b = party_slot(1, 0x99, 12, &[9, 8, 7]);
        let slot_c = party_slot(0, 0x20, 9, &[4, 5, 6]);
        let initial_left =
            GbTradePartySnapshot::new(vec![slot_a.clone(), slot_b.clone(), slot_c.clone()]);
        let initial_right = initial_left.clone();
        let final_left =
            GbTradePartySnapshot::new(vec![slot_a.clone(), slot_b.clone(), slot_a.clone()]);
        let final_right =
            GbTradePartySnapshot::new(vec![slot_b.clone(), slot_c.clone(), slot_c.clone()]);
        let validation = GbTradePartyValidation {
            initial_left: initial_left.clone(),
            initial_right: initial_right.clone(),
            final_left,
            final_right,
        };
        assert!(validation.valid_clean_trade());

        let corrupt = GbTradePartyValidation {
            initial_left: initial_left.clone(),
            initial_right: initial_right.clone(),
            final_left: initial_left,
            final_right: initial_right,
        };
        assert!(!corrupt.valid_clean_trade());
    }

    #[test]
    fn party_identity_ignores_trade_mutable_caught_data() {
        let original = party_slot(0, 0x15, 7, &[1, 2, 3]);
        let mut received_data = original.data.clone();
        received_data[29] ^= 0x7f;
        received_data[30] ^= 0x7f;
        let received = GbTradePartySlot::new(0, 6, original.species_list_entry, received_data);

        assert!(original.same_trade_identity_as(&received));
        assert_ne!(original.full_fingerprint(), received.full_fingerprint());
    }

    fn party_slot(index: u8, species: u8, level: u8, payload: &[u8]) -> GbTradePartySlot {
        let mut data = vec![0; GEN2_PARTY_MON_STRUCT_LEN];
        data[0] = species;
        data[GEN2_PARTY_MON_LEVEL_OFFSET] = level;
        data[1..1 + payload.len()].copy_from_slice(payload);
        GbTradePartySlot::new(index, 6, species, data)
    }
}
