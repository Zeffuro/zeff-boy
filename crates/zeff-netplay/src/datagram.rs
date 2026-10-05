use std::collections::BTreeMap;

use crate::{
    lockstep::Player,
    wire::{Message, PacketCodec},
};
use anyhow::{Context, Result, ensure};

const WINDOW: usize = 64;
const BATCH: usize = 32;
const HEADER: usize = 54;
const SIGNATURE: usize = 32;
const MAGIC: &[u8; 4] = b"ZND2";
const INPUT_BYTES: usize = 2;

pub struct InputChannel {
    player: Player,
    first: u64,
    sent: u64,
    acknowledged: u64,
    received: u64,
    pending: BTreeMap<u64, u16>,
    waiting: BTreeMap<u64, u16>,
    history: BTreeMap<u64, u16>,
    ack_dirty: bool,
}

impl InputChannel {
    pub fn new(player: Player, first: u64) -> Self {
        Self {
            player,
            first,
            sent: first,
            acknowledged: first,
            received: first,
            pending: BTreeMap::new(),
            waiting: BTreeMap::new(),
            history: BTreeMap::new(),
            ack_dirty: false,
        }
    }

    pub fn push(&mut self, frame: u64, buttons: u16) -> Result<()> {
        ensure!(frame == self.sent, "local input is not contiguous");
        ensure!(
            self.pending.len() < WINDOW,
            "unacknowledged input window exhausted"
        );
        let next = frame.checked_add(1).context("input frame overflow")?;
        self.pending.insert(frame, buttons);
        self.sent = next;
        Ok(())
    }

    pub fn encode(&mut self, codec: &PacketCodec) -> Result<Option<Vec<u8>>> {
        if self.pending.is_empty() && !self.ack_dirty {
            return Ok(None);
        }
        let count = self.pending.len().min(BATCH);
        let first = self
            .pending
            .first_key_value()
            .map_or(0, |(frame, _)| *frame);
        let mut bytes = Vec::with_capacity(HEADER + count * INPUT_BYTES + SIGNATURE);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&codec.transcript());
        bytes.push(role(self.player));
        bytes.extend_from_slice(&self.received.to_be_bytes());
        bytes.extend_from_slice(&first.to_be_bytes());
        bytes.push(count as u8);
        for buttons in self.pending.values().take(count) {
            bytes.extend_from_slice(&buttons.to_be_bytes());
        }
        let signature = codec.authenticate_datagram(&bytes);
        bytes.extend_from_slice(&signature);
        self.ack_dirty = false;
        Ok(Some(bytes))
    }

    pub fn receive(&mut self, codec: &PacketCodec, bytes: &[u8]) -> Result<Vec<Message>> {
        ensure!(
            (HEADER + SIGNATURE..=HEADER + BATCH * INPUT_BYTES + SIGNATURE).contains(&bytes.len()),
            "invalid input packet length"
        );
        let (body, signature) = bytes.split_at(bytes.len() - SIGNATURE);
        ensure!(
            &body[..4] == MAGIC && body[4..36] == codec.transcript(),
            "input session mismatch"
        );
        ensure!(
            body[36] == role(other(self.player)),
            "input sender role mismatch"
        );
        codec.verify_datagram(body, signature)?;
        let ack = u64::from_be_bytes(body[37..45].try_into()?);
        let first = u64::from_be_bytes(body[45..53].try_into()?);
        let count = usize::from(body[53]);
        ensure!(
            count <= BATCH && body.len() == HEADER + count * INPUT_BYTES,
            "invalid input batch length"
        );
        ensure!(
            (self.first..=self.sent).contains(&ack),
            "input acknowledgment exceeds sent frames"
        );
        ensure!(count != 0 || first == 0, "invalid empty input batch");
        let end = first
            .checked_add(count as u64)
            .context("input frame overflow")?;
        let upper = self
            .received
            .checked_add(WINDOW as u64)
            .context("input frame overflow")?;
        if count != 0 {
            ensure!(
                first >= self.first && end <= upper,
                "input exceeds receive window"
            );
        }
        let mut waiting = self.waiting.clone();
        for (offset, bytes) in body[HEADER..]
            .as_chunks::<INPUT_BYTES>()
            .0
            .iter()
            .enumerate()
        {
            let buttons = u16::from_be_bytes(*bytes);
            let frame = first + offset as u64;
            if let Some(&previous) = self.history.get(&frame).or_else(|| waiting.get(&frame)) {
                ensure!(buttons == previous, "remote input was rewritten");
            }
            if frame >= self.received {
                waiting.insert(frame, buttons);
            }
        }
        ensure!(waiting.len() <= WINDOW, "input receive window exhausted");
        self.waiting = waiting;
        self.acknowledged = self.acknowledged.max(ack);
        self.pending.retain(|frame, _| *frame >= self.acknowledged);
        let mut messages = Vec::new();
        while let Some(buttons) = self.waiting.remove(&self.received) {
            let frame = self.received;
            self.received = frame.checked_add(1).context("input frame overflow")?;
            self.history.insert(frame, buttons);
            messages.push(Message::Input {
                player: other(self.player),
                frame,
                buttons,
            });
        }
        while self.history.len() > WINDOW {
            self.history.pop_first();
        }
        self.ack_dirty |= count != 0;
        Ok(messages)
    }

    pub fn unacknowledged(&self) -> usize {
        self.pending.len()
    }

    pub fn acknowledged(&self) -> u64 {
        self.acknowledged
    }

    pub fn pending_batch(&self) -> impl Iterator<Item = u64> + '_ {
        self.pending.keys().copied().take(BATCH)
    }
}

fn role(player: Player) -> u8 {
    if player == Player::One { 1 } else { 2 }
}
fn other(player: Player) -> Player {
    if player == Player::One {
        Player::Two
    } else {
        Player::One
    }
}

#[cfg(test)]
mod tests;
