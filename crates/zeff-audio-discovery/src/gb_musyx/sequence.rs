use std::collections::{BTreeMap, BTreeSet};

use super::{
    Budget, GbMusyxTrack, ReadError,
    project::{Project, be, byte},
    require,
};

enum Event {
    Program(u8),
    Note(Option<u8>),
}

struct Track {
    number: u8,
    patterns: Vec<u8>,
    loop_start: Option<usize>,
}

type Roots = BTreeSet<(usize, u8)>;

pub(super) fn inspect(
    project: &Project<'_>,
    offset: usize,
    end: usize,
    ranges: &mut Vec<(usize, usize)>,
    budget: &mut Budget<'_>,
) -> Result<(Vec<GbMusyxTrack>, Roots), ReadError> {
    let bytes = project.bytes;
    let q = offset + 132;
    require(
        be(bytes, q)? == 6 && (1..=642).contains(&be(bytes, q + 4)?),
        "invalid sequence header",
    )?;
    let table = q + be(bytes, q + 2)?;
    require(q + 14 <= table && table + 2 <= end, "invalid pattern table")?;
    let mut tracks = Vec::new();
    let mut ids = BTreeSet::new();
    for number in 0..4 {
        budget.charge()?;
        let relative = be(bytes, q + 6 + usize::from(number) * 2)?;
        if relative == 0 {
            continue;
        }
        let start = q + relative;
        require(q + 14 <= start && start < table, "invalid track mapping")?;
        let mut at = start;
        let mut patterns = Vec::new();
        let loop_start;
        loop {
            budget.charge()?;
            require(
                at + 3 <= table && patterns.len() <= 2,
                "invalid track extent",
            )?;
            let delay = be(bytes, at)?;
            let pattern = byte(bytes, at + 2)?;
            if pattern >= 0xfe {
                require(
                    at + 7 <= table && !patterns.is_empty(),
                    "invalid track terminator",
                )?;
                if pattern == 0xfe {
                    let back = be(bytes, at + 3)?;
                    require(
                        back.is_multiple_of(3) && back / 3 < patterns.len() && delay > 0,
                        "invalid track loop",
                    )?;
                    loop_start = Some(back / 3);
                } else {
                    require(
                        be(bytes, at + 3)? == 0 && be(bytes, at + 5)? == 0,
                        "invalid track end",
                    )?;
                    loop_start = None;
                }
                ranges.push((start, at + 7));
                break;
            }
            require(pattern < 8, "invalid pattern selector")?;
            ids.insert(pattern);
            patterns.push(pattern);
            at += 3;
        }
        tracks.push(Track {
            number,
            patterns,
            loop_start,
        });
    }
    require(!tracks.is_empty(), "song has no tracks")?;
    let table_end = table
        + (usize::from(
            *ids.last()
                .ok_or(ReadError::Invalid("song has no patterns"))?,
        ) + 1)
            * 2;
    require(table_end <= end, "truncated pattern pointers")?;
    ranges.push((table, table_end));
    let mut patterns = BTreeMap::new();
    let mut tags = BTreeMap::new();
    for id in ids {
        let mut at = q + be(bytes, table + usize::from(id) * 2)?;
        let start = at;
        require(table_end <= at && at < end, "invalid pattern pointer")?;
        let mut events = Vec::new();
        loop {
            budget.charge()?;
            let event_start = at;
            require(at + 3 <= end, "truncated pattern event")?;
            let word = be(bytes, at)?;
            let key = byte(bytes, at + 2)?;
            at += 3;
            let finished = word >> 12 == 15 && key == 255;
            if !finished {
                if word >> 12 == 0 {
                    require(key & 128 != 0, "invalid program-change event")?;
                    events.push(Event::Program(key & 127));
                } else {
                    let program = if key & 128 != 0 {
                        let program = byte(bytes, at)?;
                        at += 1;
                        require(program < 128, "invalid note program")?;
                        Some(program)
                    } else {
                        None
                    };
                    let duration = byte(bytes, at)?;
                    at += 1 + usize::from(duration & 128 != 0);
                    require(at <= end, "truncated note duration")?;
                    events.push(Event::Note(program));
                }
            }
            for pos in event_start..at {
                require(
                    tags.insert(pos, event_start)
                        .is_none_or(|old| old == event_start),
                    "pattern points into an event operand",
                )?;
            }
            if finished {
                break;
            }
        }
        ranges.push((start, at));
        patterns.insert(id, events);
    }
    let mut roots = BTreeSet::new();
    let mut result = Vec::new();
    for track in tracks {
        let mut macro_id = byte(bytes, offset + usize::from(track.number))?;
        let mut notes = 0;
        let passes = track.patterns.iter().map(|id| (id, true)).chain(
            track.patterns[track.loop_start.unwrap_or(track.patterns.len())..]
                .iter()
                .map(|id| (id, false)),
        );
        for (id, first_pass) in passes {
            for event in &patterns[id] {
                budget.charge()?;
                match event {
                    Event::Program(value) => {
                        macro_id = byte(bytes, offset + 4 + usize::from(*value))?;
                    }
                    Event::Note(value) => {
                        if let Some(program) = value {
                            macro_id = byte(bytes, offset + 4 + usize::from(*program))?;
                        }
                        roots.extend(project.roots(macro_id, track.number)?);
                        notes += u32::from(first_pass);
                    }
                }
            }
        }
        result.push(GbMusyxTrack {
            number: track.number + 1,
            note_count: notes,
        });
    }
    Ok((result, roots))
}
