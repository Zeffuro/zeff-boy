use std::collections::BTreeMap;

use super::{Budget, MAX_EVENTS, NatsumeChannel, NatsumeTermination, RomSpan, ScanStop, pointer};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadError {
    Invalid(&'static str),
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

pub(super) fn error(error: impl Into<ReadError>) -> anyhow::Error {
    match error.into() {
        ReadError::Invalid(reason) => anyhow::anyhow!("Natsume music validation failed: {reason}"),
        ReadError::Stop(stop) => anyhow::anyhow!("Natsume music validation stopped: {stop:?}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ControlState {
    pc: usize,
    mode: u8,
    fixed_wait: u8,
    high_wait: u8,
    loops: [Option<usize>; 2],
    counters: [u8; 2],
    return_address: Option<usize>,
}

pub(super) fn inspect(
    bytes: &[u8],
    number: u8,
    hardware_kind: u8,
    start: usize,
    budget: &mut Budget<'_>,
) -> Result<(NatsumeChannel, Vec<RomSpan>), ReadError> {
    let mut state = ControlState {
        pc: start,
        mode: 0,
        fixed_wait: 0,
        high_wait: 0,
        loops: [None; 2],
        counters: [0; 2],
        return_address: None,
    };
    let mut channel = NatsumeChannel {
        number,
        hardware_kind,
        entry: RomSpan::new(start, 1),
        event_count: 0,
        note_count: 0,
        wait_units: 0,
        termination: NatsumeTermination::Fine,
        loop_start_wait_units: None,
    };
    let mut seen = BTreeMap::new();
    let mut spans = Vec::new();
    loop {
        budget.charge()?;
        if let Some(previous_wait) = seen.insert(state, channel.wait_units) {
            if previous_wait == channel.wait_units {
                return Err(ReadError::Invalid(
                    "sequence contains a zero-wait control cycle",
                ));
            }
            channel.termination = NatsumeTermination::Loop;
            channel.loop_start_wait_units = Some(previous_wait);
            break;
        }
        if channel.event_count as usize >= MAX_EVENTS {
            return Err(ScanStop::ValidationLimit.into());
        }
        let position = state.pc;
        let opcode = range(bytes, position, 1)?[0];
        state.pc += 1;
        let length = operand_count(opcode, state.mode, hardware_kind)?;
        let args = range(bytes, state.pc, length)?;
        for _ in 0..length {
            budget.charge()?;
        }
        state.pc += length;
        spans.push(RomSpan::new(position, length + 1));
        channel.event_count += 1;
        let mut wait = 0;
        match opcode {
            0x00..=0x7f => {
                channel.note_count += 1;
                wait = match state.mode {
                    0 | 2 => (u32::from(state.high_wait) << 8) + u32::from(args[0]) + 1,
                    1 => u32::from(state.fixed_wait),
                    _ => return Err(ReadError::Invalid("unsupported note mode")),
                };
            }
            0x80..=0x8f | 0xe0 => wait = u32::from(args[0]) + 1,
            0xe1 => state.fixed_wait = args[0],
            0xf0 | 0xf2 => {
                let slot = usize::from((opcode - 0xf0) / 2);
                state.counters[slot] = args[0];
                state.loops[slot] = Some(state.pc);
            }
            0xf1 | 0xf3 => {
                let slot = usize::from((opcode - 0xf1) / 2);
                state.counters[slot] = state.counters[slot].wrapping_sub(1);
                if state.counters[slot] != 0 {
                    state.pc = state.loops[slot]
                        .ok_or(ReadError::Invalid("loop has no initialized target"))?;
                }
            }
            0xf4 | 0xf5 => {
                let target = pointer(bytes, u32::from_le_bytes(args.try_into().unwrap()), 1, 1)?;
                if opcode == 0xf5 {
                    // This driver has one return slot; calls overwrite it, including nested calls.
                    state.return_address = Some(state.pc);
                }
                state.pc = target;
            }
            0xf6 => {
                state.pc = state
                    .return_address
                    .ok_or(ReadError::Invalid("return has no initialized target"))?;
            }
            0xfa => state.high_wait = args[0],
            0xfe => {
                if args[0] > 2 {
                    return Err(ReadError::Invalid("unsupported note mode"));
                }
                state.mode = args[0];
            }
            0xff => break,
            _ => {}
        }
        if wait != 0 {
            channel.wait_units = channel
                .wait_units
                .checked_add(wait)
                .ok_or(ReadError::Invalid("structural wait count overflow"))?;
            // The next driver tick clears the high-byte prefix, including after a rest.
            state.high_wait = 0;
        }
    }
    merge_spans(&mut spans);
    Ok((channel, spans))
}

fn operand_count(opcode: u8, mode: u8, hardware_kind: u8) -> Result<usize, ReadError> {
    Ok(match opcode {
        0x00..=0x7f => match mode {
            0 | 2 => 1,
            1 if hardware_kind == 3 => 0,
            1 => 1,
            _ => return Err(ReadError::Invalid("unsupported note mode")),
        },
        0x80..=0x8f | 0xe0..=0xe3 | 0xe8..=0xea | 0xf0 | 0xf2 | 0xf7 | 0xf8 | 0xfa | 0xfe => 1,
        0x90..=0xdf | 0xe4..=0xe7 | 0xeb | 0xf1 | 0xf3 | 0xf6 | 0xfc | 0xfd | 0xff => 0,
        0xf9 => 2,
        0xf4 | 0xf5 | 0xfb => 4,
        _ => return Err(ReadError::Invalid("unsupported sequence opcode")),
    })
}

pub(super) fn range(bytes: &[u8], offset: usize, len: usize) -> Result<&[u8], ReadError> {
    offset
        .checked_add(len)
        .and_then(|end| bytes.get(offset..end))
        .ok_or(ReadError::Invalid(
            "sequence or structure leaves the source",
        ))
}

pub(super) fn merge_spans(spans: &mut Vec<RomSpan>) {
    spans.sort_unstable();
    let mut retained: Vec<RomSpan> = Vec::new();
    for span in spans.drain(..) {
        if let Some(previous) = retained.last_mut() {
            let end = previous.effective_offset + previous.byte_len;
            if span.effective_offset <= end {
                previous.byte_len =
                    end.max(span.effective_offset + span.byte_len) - previous.effective_offset;
                continue;
            }
        }
        retained.push(span);
    }
    *spans = retained;
}
