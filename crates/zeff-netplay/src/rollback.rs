use std::collections::{BTreeMap, VecDeque};

use anyhow::{Result, bail, ensure};

use crate::lockstep::Player;

pub const PREDICTION_WINDOW: u64 = 8;
const RETAINED_INPUTS: u64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDelay(u64);

impl InputDelay {
    pub const MIN: u64 = 0;
    pub const MAX: u64 = 8;
    pub const DEFAULT: u64 = 2;

    pub fn new(frames: u64) -> Result<Self> {
        ensure!(
            (Self::MIN..=Self::MAX).contains(&frames),
            "input delay must be between 0 and 8 frames"
        );
        Ok(Self(frames))
    }

    pub fn frames(self) -> u64 {
        self.0
    }

    pub fn lookahead(self) -> u64 {
        self.pause_lead() + 1
    }

    pub fn pause_lead(self) -> u64 {
        PREDICTION_WINDOW + 2 * self.0
    }
}

impl Default for InputDelay {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInput {
    pub frame: u64,
    pub ports: [u8; 2],
}

pub struct Timeline {
    player: Player,
    input_delay: InputDelay,
    frame: u64,
    confirmed: u64,
    inputs: BTreeMap<u64, [Option<u8>; 2]>,
    executed: VecDeque<FrameInput>,
    base_ports: [u8; 2],
    dirty: Option<u64>,
}

fn port(player: Player) -> usize {
    match player {
        Player::One => 0,
        Player::Two => 1,
    }
}

impl Timeline {
    pub fn new(player: Player) -> Self {
        Self::with_delay(player, InputDelay::default())
    }

    pub fn with_delay(player: Player, input_delay: InputDelay) -> Self {
        Self {
            player,
            input_delay,
            frame: 0,
            confirmed: input_delay.frames(),
            inputs: (0..input_delay.frames())
                .map(|frame| (frame, [Some(0); 2]))
                .collect(),
            executed: VecDeque::new(),
            base_ports: [0; 2],
            dirty: None,
        }
    }

    pub fn input_delay(&self) -> InputDelay {
        self.input_delay
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn confirmed_frame(&self) -> u64 {
        self.confirmed.min(self.frame)
    }

    pub fn prediction_depth(&self) -> u64 {
        self.frame.saturating_sub(self.confirmed)
    }

    pub fn retained_inputs(&self) -> usize {
        self.inputs.len()
    }

    pub fn sample_local(&mut self, buttons: u8) -> Result<(u64, u8)> {
        let frame = self
            .frame
            .checked_add(self.input_delay.frames())
            .ok_or_else(|| anyhow::anyhow!("frame overflow"))?;
        let entry = self.inputs.entry(frame).or_default();
        let value = *entry[port(self.player)].get_or_insert(buttons);
        self.update_confirmation()?;
        Ok((frame, value))
    }

    pub fn receive_remote(&mut self, frame: u64, buttons: u8) -> Result<bool> {
        let upper = self
            .frame
            .checked_add(self.input_delay.lookahead())
            .ok_or_else(|| anyhow::anyhow!("frame overflow"))?;
        ensure!(frame <= upper, "remote input exceeds prediction window");
        let oldest = self.inputs.first_key_value().map_or(0, |(frame, _)| *frame);
        if frame < oldest {
            return Ok(false);
        }
        let peer = 1 - port(self.player);
        let entry = self.inputs.entry(frame).or_default();
        if let Some(previous) = entry[peer] {
            ensure!(previous == buttons, "remote input was rewritten");
            return Ok(false);
        }
        entry[peer] = Some(buttons);
        if self
            .executed
            .iter()
            .any(|used| used.frame == frame && used.ports[peer] != buttons)
        {
            self.dirty = Some(self.dirty.map_or(frame, |dirty| dirty.min(frame)));
        }
        self.update_confirmation()?;
        Ok(true)
    }

    fn update_confirmation(&mut self) -> Result<()> {
        while self
            .inputs
            .get(&self.confirmed)
            .is_some_and(|input| input.iter().all(Option::is_some))
        {
            self.confirmed = self
                .confirmed
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("frame overflow"))?;
        }
        Ok(())
    }

    pub fn next_frame(&self) -> Result<Option<FrameInput>> {
        ensure!(self.dirty.is_none(), "correction must precede simulation");
        if self.frame.saturating_sub(self.confirmed) >= PREDICTION_WINDOW {
            return Ok(None);
        }
        Ok(Some(FrameInput {
            frame: self.frame,
            ports: self.ports_at(self.frame),
        }))
    }

    pub fn advance(&mut self, input: FrameInput) -> Result<()> {
        ensure!(self.next_frame()? == Some(input), "incorrect frame input");
        self.frame = self
            .frame
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("frame overflow"))?;
        self.executed.push_back(input);
        self.retire();
        Ok(())
    }

    pub fn correction(&self) -> Result<Vec<FrameInput>> {
        let Some(dirty) = self.dirty else {
            return Ok(Vec::new());
        };
        ensure!(
            self.frame.saturating_sub(dirty) <= PREDICTION_WINDOW,
            "correction exceeds retained window"
        );
        Ok((dirty..self.frame)
            .map(|frame| FrameInput {
                frame,
                ports: self.ports_at(frame),
            })
            .collect())
    }

    pub fn corrected(&mut self, replayed: &[FrameInput]) -> Result<()> {
        ensure!(
            replayed == self.correction()?,
            "incorrect correction trajectory"
        );
        for replacement in replayed {
            let Some(used) = self
                .executed
                .iter_mut()
                .find(|used| used.frame == replacement.frame)
            else {
                bail!("missing executed frame");
            };
            *used = *replacement;
        }
        self.dirty = None;
        self.retire();
        Ok(())
    }

    fn ports_at(&self, frame: u64) -> [u8; 2] {
        let mut ports = self.base_ports;
        for index in 0..2 {
            if let Some(value) = self
                .inputs
                .range(..=frame)
                .rev()
                .find_map(|(_, input)| input[index])
            {
                ports[index] = value;
            }
        }
        ports
    }

    fn retire(&mut self) {
        let confirmed = self.confirmed_frame();
        while self
            .executed
            .front()
            .is_some_and(|used| used.frame < confirmed)
        {
            self.executed.pop_front();
        }
        let oldest = confirmed.saturating_sub(RETAINED_INPUTS);
        self.base_ports = self.ports_at(oldest);
        self.inputs.retain(|&frame, _| frame >= oldest);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod wan_tests;
