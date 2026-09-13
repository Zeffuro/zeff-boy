use anyhow::{Context, Result, bail, ensure};

use super::mp2k::{self, Event, Program};

const MAX_TIMELINE_EVENTS: usize = 1_000_000;
pub(super) const GBA_CLOCK_HZ: u128 = 16_777_216;
pub(super) const GBA_CYCLES_PER_FRAME: u128 = 280_896;

#[derive(Clone, Debug)]
pub(super) enum ScheduledEvent {
    Sequence(Event),
    GateOff { key: u8 },
}

#[derive(Clone, Debug)]
pub(super) struct Scheduled {
    pub(super) tick: u32,
    pub(super) track: u8,
    pub(super) order: u32,
    pub(super) event: ScheduledEvent,
}

fn expanded_end(program: &Program, loops: u8) -> Result<u32> {
    match (program.loop_start, program.loop_event_start) {
        (Some(start), Some(index)) => {
            ensure!(
                index <= program.events.len(),
                "loop event index is out of range"
            );
            let loop_ticks = program.ticks - start;
            start
                .checked_add(
                    loop_ticks
                        .checked_mul(u32::from(loops))
                        .context("loop duration overflows")?,
                )
                .context("loop duration overflows")
        }
        (None, None) => Ok(program.ticks),
        _ => bail!("sequence loop metadata is inconsistent"),
    }
}

pub(super) fn build_timeline(programs: &[Program], loops: u8) -> Result<(Vec<Scheduled>, u32)> {
    let song_end = programs
        .iter()
        .map(|program| expanded_end(program, loops))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    ensure!(song_end > 0, "song has zero duration");
    let mut timeline = Vec::new();
    let mut order = 0u32;
    for (track_index, program) in programs.iter().enumerate() {
        let track = u8::try_from(track_index).context("track index overflows")?;
        match (program.loop_start, program.loop_event_start) {
            (None, None) => append_events(
                &mut timeline,
                &program.events,
                track,
                0,
                0,
                song_end,
                &mut order,
            )?,
            (Some(start), Some(index)) => {
                let (intro, loop_body) = program
                    .events
                    .split_at_checked(index)
                    .context("loop event index is out of range")?;
                append_events(&mut timeline, intro, track, 0, 0, song_end, &mut order)?;
                let loop_ticks = program.ticks - start;
                if loop_body.is_empty() {
                    continue;
                }
                let repeats = (song_end - start).div_ceil(loop_ticks) as usize;
                ensure!(
                    loop_body
                        .len()
                        .checked_mul(repeats)
                        .and_then(|count| count.checked_add(timeline.len()))
                        .is_some_and(|count| count <= MAX_TIMELINE_EVENTS),
                    "expanded sequence exceeds the event limit"
                );
                let mut base = start;
                while base < song_end {
                    append_events(
                        &mut timeline,
                        loop_body,
                        track,
                        base,
                        start,
                        song_end,
                        &mut order,
                    )?;
                    base = base
                        .checked_add(loop_ticks)
                        .context("loop expansion overflows")?;
                }
            }
            _ => bail!("sequence loop metadata is inconsistent"),
        }
    }
    ensure!(
        timeline.len() <= MAX_TIMELINE_EVENTS,
        "expanded sequence exceeds the event limit"
    );
    let mut gate_offs = Vec::new();
    for item in &timeline {
        if let ScheduledEvent::Sequence(Event::Note { key, gate, .. }) = item.event
            && gate != 0
        {
            let tick = item
                .tick
                .checked_add(u32::from(gate))
                .context("note gate overflows")?;
            if tick <= song_end {
                gate_offs.push(Scheduled {
                    tick,
                    track: item.track,
                    order: item.order,
                    event: ScheduledEvent::GateOff { key },
                });
            }
        }
    }
    timeline.extend(gate_offs);
    ensure!(
        timeline.len() <= MAX_TIMELINE_EVENTS,
        "expanded sequence exceeds the event limit"
    );
    timeline.sort_by_key(|item| {
        let phase = matches!(item.event, ScheduledEvent::Sequence(_)) as u8;
        (item.tick, phase, item.track, item.order)
    });
    Ok((timeline, song_end))
}

#[allow(clippy::too_many_arguments)]
fn append_events(
    output: &mut Vec<Scheduled>,
    events: &[mp2k::TimedEvent],
    track: u8,
    destination_start: u32,
    source_start: u32,
    song_end: u32,
    order: &mut u32,
) -> Result<()> {
    for timed in events {
        let relative_tick = timed
            .tick
            .checked_sub(source_start)
            .context("loop event precedes its loop start")?;
        let tick = destination_start
            .checked_add(relative_tick)
            .context("event time overflows")?;
        if tick > song_end || (tick == song_end && matches!(timed.event, Event::Note { .. })) {
            continue;
        }
        ensure!(
            output.len() < MAX_TIMELINE_EVENTS,
            "expanded sequence exceeds the event limit"
        );
        output.push(Scheduled {
            tick,
            track,
            order: *order,
            event: ScheduledEvent::Sequence(timed.event.clone()),
        });
        *order = order.checked_add(1).context("event order overflows")?;
    }
    Ok(())
}

pub(super) fn advance_time(current: u128, ticks: u32, tempo: u8, numerator: u128) -> Result<u128> {
    ensure!(
        tempo != 0,
        "sequence contains a zero tempo that stops sequence time"
    );
    let increment = u128::from(ticks)
        .checked_mul(numerator)
        .and_then(|value| value.checked_shl(32))
        .context("sequence time overflows")?
        / (u128::from(tempo) * GBA_CLOCK_HZ);
    current
        .checked_add(increment)
        .context("sequence time overflows")
}
