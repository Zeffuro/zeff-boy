use anyhow::{Context, Result, bail};

use super::{MetadataCursor, ReplayEvent, write_u32};

const MAGIC: &[u8; 4] = b"ZREV";
const VERSION: u32 = 1;
const MAX_EVENTS: usize = 100_000;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const METADATA_VERSION: u32 = 3;

pub fn encode_replay_event_stream(events: &[ReplayEvent]) -> Result<Vec<u8>> {
    if events.len() > MAX_EVENTS {
        bail!("replay event stream exceeds {MAX_EVENTS} events");
    }

    let mut events = events.to_vec();
    events.sort_by(ReplayEvent::canonical_cmp);
    encode_canonical_replay_event_stream(&events)
}

pub fn encode_canonical_replay_event_stream(events: &[ReplayEvent]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if events.len() > MAX_EVENTS {
        bail!("replay event stream exceeds {MAX_EVENTS} events");
    }
    if events
        .windows(2)
        .any(|pair| pair[0].canonical_cmp(&pair[1]).is_gt())
    {
        bail!("replay event stream is not in canonical order");
    }
    out.extend_from_slice(MAGIC);
    write_u32(&mut out, VERSION);
    write_u32(&mut out, events.len() as u32);
    for event in events {
        let mut bytes = Vec::new();
        event.encode(&mut bytes, METADATA_VERSION);
        if bytes.len() > MAX_EVENT_BYTES {
            bail!("encoded replay event exceeds {MAX_EVENT_BYTES} bytes");
        }
        write_u32(&mut out, bytes.len() as u32);
        out.extend_from_slice(&bytes);
        if out.len() > MAX_STREAM_BYTES {
            bail!("replay event stream exceeds {MAX_STREAM_BYTES} bytes");
        }
    }
    Ok(out)
}

pub fn decode_replay_event_stream(bytes: &[u8]) -> Result<Vec<ReplayEvent>> {
    if bytes.len() > MAX_STREAM_BYTES {
        bail!("replay event stream exceeds {MAX_STREAM_BYTES} bytes");
    }
    let mut cursor = MetadataCursor::new(bytes);
    if cursor.read_exact(4)? != MAGIC {
        bail!("invalid replay event stream magic");
    }
    let version = cursor.read_u32()?;
    if version != VERSION {
        bail!("unsupported replay event stream version: {version}");
    }
    let count = cursor.read_u32()? as usize;
    if count > MAX_EVENTS {
        bail!("replay event stream exceeds {MAX_EVENTS} events");
    }

    let mut events = Vec::with_capacity(count);
    for index in 0..count {
        let len = cursor.read_u32()? as usize;
        if len > MAX_EVENT_BYTES {
            bail!("replay event {index} exceeds {MAX_EVENT_BYTES} bytes");
        }
        let event_bytes = cursor
            .read_exact(len)
            .with_context(|| format!("truncated replay event {index}"))?;
        let mut event_cursor = MetadataCursor::new(event_bytes);
        let event = ReplayEvent::decode(&mut event_cursor, METADATA_VERSION)
            .with_context(|| format!("invalid replay event {index}"))?;
        event_cursor
            .finish()
            .with_context(|| format!("invalid replay event {index}"))?;
        events.push(event);
    }
    cursor.finish()?;

    let mut canonical = events.clone();
    canonical.sort_by(ReplayEvent::canonical_cmp);
    if events != canonical {
        bail!("replay event stream is not in canonical order");
    }
    Ok(events)
}
