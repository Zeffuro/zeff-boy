use std::collections::{BTreeMap, VecDeque};

pub const INPUT_DELAY: u64 = 2;
pub const MAX_AHEAD: u64 = 8;
pub const HISTORY_LIMIT: usize = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Player {
    One,
    Two,
}

impl Player {
    fn port(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfirmedInput {
    pub frame: u64,
    pub ports: [u8; 2],
}

#[derive(Debug)]
pub struct Lockstep {
    frame: u64,
    pending: BTreeMap<u64, [Option<u8>; 2]>,
    history: VecDeque<ConfirmedInput>,
    closed: bool,
}

impl Default for Lockstep {
    fn default() -> Self {
        Self::new()
    }
}

impl Lockstep {
    pub fn new() -> Self {
        let pending = (0..INPUT_DELAY)
            .map(|frame| (frame, [Some(0), Some(0)]))
            .collect();
        Self {
            frame: 0,
            pending,
            history: VecDeque::new(),
            closed: false,
        }
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn submit(&mut self, player: Player, frame: u64, buttons: u8) -> Result<bool, String> {
        self.ensure_open()?;
        if frame < INPUT_DELAY && buttons != 0 {
            return Err("bootstrap input must be neutral".into());
        }
        let port = player.port();
        if frame < self.frame {
            return match self.history.iter().find(|input| input.frame == frame) {
                Some(input) if input.ports[port] != buttons => {
                    Err("input conflicts with confirmed history".into())
                }
                _ => Ok(false),
            };
        }
        if frame - self.frame > MAX_AHEAD {
            return Err("input exceeds the pending frame window".into());
        }
        let ports = self.pending.entry(frame).or_insert([None, None]);
        match ports[port] {
            Some(existing) if existing != buttons => {
                Err("input conflicts with pending input".into())
            }
            Some(_) => Ok(false),
            None => {
                ports[port] = Some(buttons);
                Ok(true)
            }
        }
    }

    pub fn advance(&mut self) -> Result<Option<ConfirmedInput>, String> {
        self.ensure_open()?;
        let Some([Some(one), Some(two)]) = self.pending.get(&self.frame).copied() else {
            return Ok(None);
        };
        let next = self
            .frame
            .checked_add(1)
            .ok_or_else(|| "frame counter overflow".to_string())?;
        let input = ConfirmedInput {
            frame: self.frame,
            ports: [one, two],
        };
        self.pending.remove(&self.frame);
        self.frame = next;
        self.history.push_back(input);
        if self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
        Ok(Some(input))
    }

    pub fn history(&self) -> &VecDeque<ConfirmedInput> {
        &self.history
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.pending.clear();
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self.closed {
            Err("lockstep session is closed".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advance_bootstrap(lockstep: &mut Lockstep) {
        for frame in 0..INPUT_DELAY {
            assert_eq!(
                lockstep.advance().unwrap(),
                Some(ConfirmedInput {
                    frame,
                    ports: [0, 0],
                })
            );
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Snapshot {
        frame: u64,
        pending: BTreeMap<u64, [Option<u8>; 2]>,
        history: VecDeque<ConfirmedInput>,
    }

    fn snapshot(lockstep: &Lockstep) -> Snapshot {
        Snapshot {
            frame: lockstep.frame,
            pending: lockstep.pending.clone(),
            history: lockstep.history.clone(),
        }
    }

    #[test]
    fn two_frame_delay_and_owned_ports_merge_in_order() {
        let mut lockstep = Lockstep::new();
        assert_eq!(lockstep.frame(), 0);
        assert!(lockstep.submit(Player::Two, INPUT_DELAY, 0x80).unwrap());
        assert!(lockstep.submit(Player::One, INPUT_DELAY, 0x01).unwrap());
        advance_bootstrap(&mut lockstep);
        assert_eq!(
            lockstep.advance().unwrap(),
            Some(ConfirmedInput {
                frame: 2,
                ports: [0x01, 0x80],
            })
        );
        assert_eq!(lockstep.frame(), 3);
        assert_eq!(lockstep.history().len(), 3);
        assert_eq!(lockstep.advance().unwrap(), None);
    }

    #[test]
    fn missing_port_blocks_without_fabricating_or_consuming_input() {
        let mut lockstep = Lockstep::new();
        advance_bootstrap(&mut lockstep);
        assert!(lockstep.submit(Player::One, 2, 7).unwrap());
        assert!(lockstep.submit(Player::One, 3, 8).unwrap());
        assert!(lockstep.submit(Player::Two, 3, 9).unwrap());
        let before = snapshot(&lockstep);
        assert_eq!(lockstep.advance().unwrap(), None);
        assert_eq!(snapshot(&lockstep), before);
        assert!(lockstep.submit(Player::Two, 2, 6).unwrap());
        assert_eq!(lockstep.advance().unwrap().unwrap().ports, [7, 6]);
        assert_eq!(lockstep.advance().unwrap().unwrap().ports, [8, 9]);
        assert_eq!(lockstep.advance().unwrap(), None);
    }

    #[test]
    fn duplicates_are_idempotent_and_conflicts_leave_state_intact() {
        let mut lockstep = Lockstep::new();
        assert!(lockstep.submit(Player::One, 2, 3).unwrap());
        let before = snapshot(&lockstep);
        assert!(!lockstep.submit(Player::One, 2, 3).unwrap());
        assert!(lockstep.submit(Player::One, 2, 4).is_err());
        assert_eq!(snapshot(&lockstep), before);
        assert!(lockstep.submit(Player::Two, 2, 4).unwrap());
        advance_bootstrap(&mut lockstep);
        lockstep.advance().unwrap();
        let before = snapshot(&lockstep);
        assert!(!lockstep.submit(Player::One, 2, 3).unwrap());
        assert!(!lockstep.submit(Player::Two, 2, 4).unwrap());
        assert!(lockstep.submit(Player::One, 2, 4).is_err());
        assert_eq!(snapshot(&lockstep), before);
    }

    #[test]
    fn bootstrap_neutrality_is_enforced_before_and_after_confirmation() {
        let mut lockstep = Lockstep::new();
        for frame in 0..INPUT_DELAY {
            for player in [Player::One, Player::Two] {
                let before = snapshot(&lockstep);
                assert!(!lockstep.submit(player, frame, 0).unwrap());
                assert!(lockstep.submit(player, frame, 1).is_err());
                assert_eq!(snapshot(&lockstep), before);
            }
        }
        advance_bootstrap(&mut lockstep);
        let before = snapshot(&lockstep);
        assert!(lockstep.submit(Player::Two, 1, 1).is_err());
        assert_eq!(snapshot(&lockstep), before);
    }

    #[test]
    fn pending_window_is_inclusive_bounded_and_moves_with_current_frame() {
        let mut lockstep = Lockstep::new();
        for frame in 2..=MAX_AHEAD {
            assert!(lockstep.submit(Player::One, frame, 1).unwrap());
        }
        assert_eq!(lockstep.pending.len(), MAX_AHEAD as usize + 1);
        let before = snapshot(&lockstep);
        assert!(lockstep.submit(Player::Two, MAX_AHEAD + 1, 1).is_err());
        assert_eq!(snapshot(&lockstep), before);
        lockstep.advance().unwrap();
        assert!(lockstep.submit(Player::Two, MAX_AHEAD + 1, 1).unwrap());
        assert_eq!(lockstep.pending.len(), MAX_AHEAD as usize + 1);
    }

    #[test]
    fn retained_stale_inputs_require_equality_and_older_inputs_are_ignored() {
        let mut lockstep = Lockstep::new();
        advance_bootstrap(&mut lockstep);
        for frame in 2..=122 {
            lockstep.submit(Player::One, frame, 3).unwrap();
            lockstep.submit(Player::Two, frame, 4).unwrap();
            lockstep.advance().unwrap();
        }
        assert_eq!(lockstep.history().len(), HISTORY_LIMIT);
        assert_eq!(lockstep.history().front().unwrap().frame, 3);
        let before = snapshot(&lockstep);
        assert!(!lockstep.submit(Player::One, 2, 0xff).unwrap());
        assert!(!lockstep.submit(Player::Two, 3, 4).unwrap());
        assert!(lockstep.submit(Player::Two, 3, 0xff).is_err());
        assert!(lockstep.submit(Player::One, 0, 1).is_err());
        assert_eq!(snapshot(&lockstep), before);
    }

    #[test]
    fn near_maximum_frame_accepts_only_representable_inputs_and_overflow_is_atomic() {
        let mut lockstep = Lockstep {
            frame: u64::MAX - 1,
            pending: BTreeMap::new(),
            history: VecDeque::new(),
            closed: false,
        };
        for frame in [u64::MAX - 1, u64::MAX] {
            lockstep.submit(Player::One, frame, 1).unwrap();
            lockstep.submit(Player::Two, frame, 2).unwrap();
        }
        assert_eq!(lockstep.advance().unwrap().unwrap().frame, u64::MAX - 1);
        let before = snapshot(&lockstep);
        assert!(lockstep.advance().is_err());
        assert_eq!(snapshot(&lockstep), before);
        assert!(!lockstep.submit(Player::One, u64::MAX, 1).unwrap());
    }

    #[test]
    fn closed_session_drops_pending_and_rejects_all_further_operations() {
        let mut lockstep = Lockstep::default();
        lockstep.advance().unwrap();
        lockstep.submit(Player::One, 2, 3).unwrap();
        lockstep.close();
        assert!(lockstep.pending.is_empty());
        let before = snapshot(&lockstep);
        for frame in [0, 1, 2, u64::MAX] {
            assert!(lockstep.submit(Player::One, frame, 0).is_err());
        }
        assert!(lockstep.advance().is_err());
        lockstep.close();
        assert_eq!(snapshot(&lockstep), before);
    }
}
