use super::*;

pub(super) const HASH_INTERVAL: u64 = 60;
const CAPACITY: usize = 8;

#[derive(Default)]
pub(super) struct Hashes {
    expected: BTreeMap<u64, Message>,
    pending: BTreeMap<u64, Message>,
    matched: Option<Message>,
}

impl Hashes {
    pub(super) fn local(&mut self, message: Message) -> Result<()> {
        let frame = periodic_frame(&message)?;
        ensure!(
            !self.expected.contains_key(&frame),
            "duplicate local checkpoint"
        );
        if let Some(remote) = self.pending.remove(&frame) {
            ensure!(
                remote == message,
                "netplay checkpoint differs at frame {frame}"
            );
            self.matched = Some(message);
        } else {
            ensure!(self.expected.len() < CAPACITY, "peer checkpoint stalled");
            self.expected.insert(frame, message);
        }
        Ok(())
    }

    pub(super) fn remote(&mut self, message: Message, current: u64, lookahead: u64) -> Result<()> {
        let frame = periodic_frame(&message)?;
        ensure!(
            frame
                <= current
                    .checked_add(lookahead)
                    .context("checkpoint frame overflow")?,
            "peer checkpoint exceeds window"
        );
        if let Some(matched) = &self.matched
            && frame <= frame_of(matched)
        {
            ensure!(message == *matched, "stale or changed peer checkpoint");
            return Ok(());
        }
        if let Some(local) = self.expected.remove(&frame) {
            ensure!(
                local == message,
                "netplay checkpoint differs at frame {frame}"
            );
            self.matched = Some(message);
        } else if let Some(previous) = self.pending.get(&frame) {
            ensure!(*previous == message, "peer changed a pending checkpoint");
        } else {
            ensure!(
                self.pending.len() < CAPACITY,
                "peer checkpoint queue is full"
            );
            self.pending.insert(frame, message);
        }
        Ok(())
    }
}

fn periodic_frame(message: &Message) -> Result<u64> {
    let Message::Checkpoint { frame, .. } = message else {
        anyhow::bail!("expected checkpoint");
    };
    ensure!(
        *frame != 0 && *frame % HASH_INTERVAL == 0,
        "invalid periodic checkpoint frame"
    );
    Ok(*frame)
}

fn frame_of(message: &Message) -> u64 {
    match message {
        Message::Checkpoint { frame, .. } => *frame,
        _ => unreachable!(),
    }
}
