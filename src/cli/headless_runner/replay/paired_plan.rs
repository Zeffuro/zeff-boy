use std::collections::HashMap;
use std::fmt;

use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply,
    ReplayPlayer,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Side {
    Left,
    Right,
}

impl Side {
    pub(super) fn peer(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Point {
    pub(super) frame: u64,
    pub(super) tick: u64,
    pub(super) absolute_tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocatedEvent {
    pub(super) ordinal: usize,
    pub(super) point: Point,
    pub(super) event: ReplayGameBoyLinkEvent,
}

impl LocatedEvent {
    pub(super) fn action(self) -> ReplayGameBoyLinkAction {
        action(self.event).expect("transfer start has an action")
    }

    pub(super) fn reply(self) -> ReplayGameBoyLinkReply {
        reply(self.event).expect("transfer reply has reply data")
    }
}

#[derive(Clone, Default)]
struct EndpointTransfer {
    local: Option<LocatedEvent>,
    remote: Option<LocatedEvent>,
    reply: Option<LocatedEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Transfer {
    pub(super) id: u64,
    pub(super) master: Side,
    pub(super) left_start: LocatedEvent,
    pub(super) right_start: LocatedEvent,
    pub(super) master_reply: LocatedEvent,
}

impl Transfer {
    pub(super) fn start(&self, side: Side) -> LocatedEvent {
        match side {
            Side::Left => self.left_start,
            Side::Right => self.right_start,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TransferBatch {
    pub(super) transfers: Vec<Transfer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InitialMasterContinuation {
    pub(super) master: Side,
    pub(super) state: ReplayGameBoyLinkCoordinatorState,
    pub(super) reply_event: Option<LocatedEvent>,
}

#[derive(Clone, Debug)]
pub(super) struct PairedTransferPlan {
    pub(super) anchor_id: u64,
    pub(super) left_anchor_tick: u64,
    pub(super) right_anchor_tick: u64,
    pub(super) batches: Vec<TransferBatch>,
    pub(super) initial_master: Option<InitialMasterContinuation>,
}

pub(super) fn validate_paired_transfer_plan(
    left: &ReplayPlayer,
    right: &ReplayPlayer,
) -> Result<PairedTransferPlan, PairedPlanError> {
    let initial_master = validate_initial_master_continuation(left, right)?;
    let ignored_reply = initial_master.and_then(|initial| {
        initial
            .reply_event
            .map(|event| (initial.master, event.ordinal))
    });
    let plan = if !has_link_events_except(&left.metadata().events, Side::Left, ignored_reply)
        && !has_link_events_except(&right.metadata().events, Side::Right, ignored_reply)
        && (left.metadata().game_boy_link_start_state.is_some()
            || right.metadata().game_boy_link_start_state.is_some())
    {
        let mut plan = PairedTransferPlan::start_only(
            left.metadata().game_boy_link_start_tick,
            right.metadata().game_boy_link_start_tick,
        )?;
        plan.initial_master = initial_master;
        plan
    } else {
        PairedTransferPlan::build_with_initial(
            &left.metadata().events,
            left.metadata().game_boy_link_start_tick,
            &right.metadata().events,
            right.metadata().game_boy_link_start_tick,
            initial_master,
        )?
    };
    log::debug!(
        "paired GB replay plan: anchor={:#018X} left_tick={} right_tick={} batches={}",
        plan.anchor_id,
        plan.left_anchor_tick,
        plan.right_anchor_tick,
        plan.batches.len()
    );
    Ok(plan)
}

impl PairedTransferPlan {
    pub(super) fn start_only(
        left_start_tick: Option<u64>,
        right_start_tick: Option<u64>,
    ) -> Result<Self, PairedPlanError> {
        Ok(Self {
            anchor_id: 0,
            left_anchor_tick: left_start_tick
                .ok_or(PairedPlanError::MissingStartTick { side: Side::Left })?,
            right_anchor_tick: right_start_tick
                .ok_or(PairedPlanError::MissingStartTick { side: Side::Right })?,
            batches: Vec::new(),
            initial_master: None,
        })
    }

    #[cfg(test)]
    pub(super) fn build(
        left_events: &[ReplayEvent],
        left_start_tick: Option<u64>,
        right_events: &[ReplayEvent],
        right_start_tick: Option<u64>,
    ) -> Result<Self, PairedPlanError> {
        Self::build_with_initial(
            left_events,
            left_start_tick,
            right_events,
            right_start_tick,
            None,
        )
    }

    fn build_with_initial(
        left_events: &[ReplayEvent],
        left_start_tick: Option<u64>,
        right_events: &[ReplayEvent],
        right_start_tick: Option<u64>,
        initial_master: Option<InitialMasterContinuation>,
    ) -> Result<Self, PairedPlanError> {
        let ignored_reply = initial_master.and_then(|initial| {
            initial
                .reply_event
                .map(|event| (initial.master, event.ordinal))
        });
        if !has_link_events_except(left_events, Side::Left, ignored_reply)
            && !has_link_events_except(right_events, Side::Right, ignored_reply)
        {
            return Err(PairedPlanError::Empty);
        }
        let left_start_tick =
            left_start_tick.ok_or(PairedPlanError::MissingStartTick { side: Side::Left })?;
        let right_start_tick =
            right_start_tick.ok_or(PairedPlanError::MissingStartTick { side: Side::Right })?;
        let left = index_endpoint(Side::Left, left_events, left_start_tick, ignored_reply)?;
        let right = index_endpoint(Side::Right, right_events, right_start_tick, ignored_reply)?;

        let mut ids: Vec<_> = left.keys().chain(right.keys()).copied().collect();
        ids.sort_unstable();
        ids.dedup();
        let mut transfers = Vec::with_capacity(ids.len());
        for id in ids {
            transfers.push(validate_transfer(id, left.get(&id), right.get(&id))?);
        }
        validate_endpoint_ids(&transfers, initial_master)?;

        let anchor = first_common_transfer_in_stream_order(right_events, &transfers)
            .ok_or(PairedPlanError::Empty)?;
        let batches = build_causal_batches(&transfers)?;
        let first_batch = batches.first().ok_or(PairedPlanError::Empty)?;
        let (left_anchor_tick, right_anchor_tick) = (
            first_batch
                .transfers
                .iter()
                .min_by_key(|transfer| transfer.left_start.ordinal)
                .expect("a transfer batch is non-empty")
                .left_start
                .point
                .absolute_tick,
            first_batch
                .transfers
                .iter()
                .min_by_key(|transfer| transfer.right_start.ordinal)
                .expect("a transfer batch is non-empty")
                .right_start
                .point
                .absolute_tick,
        );
        Ok(Self {
            anchor_id: anchor.id,
            left_anchor_tick,
            right_anchor_tick,
            batches,
            initial_master,
        })
    }
}

fn has_link_events_except(
    events: &[ReplayEvent],
    side: Side,
    ignored_reply: Option<(Side, usize)>,
) -> bool {
    events.iter().enumerate().any(|(ordinal, event)| {
        matches!(event, ReplayEvent::GameBoyLink { .. }) && ignored_reply != Some((side, ordinal))
    })
}

fn index_endpoint(
    side: Side,
    events: &[ReplayEvent],
    start_tick: u64,
    ignored_reply: Option<(Side, usize)>,
) -> Result<HashMap<u64, EndpointTransfer>, PairedPlanError> {
    let mut indexed = HashMap::<u64, EndpointTransfer>::new();
    for (ordinal, event) in events.iter().enumerate() {
        if ignored_reply == Some((side, ordinal)) {
            continue;
        }
        let ReplayEvent::GameBoyLink { frame, tick, event } = event else {
            continue;
        };
        let point = Point {
            frame: *frame,
            tick: *tick,
            absolute_tick: start_tick
                .checked_add(*tick)
                .ok_or(PairedPlanError::TickOverflow { side, tick: *tick })?,
        };
        let (id, slot, role) = match event {
            ReplayGameBoyLinkEvent::LocalMasterStart { transfer_id, .. } => (
                *transfer_id,
                &mut indexed.entry(*transfer_id).or_default().local,
                "local master start",
            ),
            ReplayGameBoyLinkEvent::RemoteMasterStart { transfer_id, .. } => (
                *transfer_id,
                &mut indexed.entry(*transfer_id).or_default().remote,
                "remote master start",
            ),
            ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } => (
                *transfer_id,
                &mut indexed.entry(*transfer_id).or_default().reply,
                "remote reply",
            ),
        };
        if slot
            .replace(LocatedEvent {
                ordinal,
                point,
                event: *event,
            })
            .is_some()
        {
            return Err(PairedPlanError::DuplicateRole { side, id, role });
        }
    }
    Ok(indexed)
}

fn validate_transfer(
    id: u64,
    left: Option<&EndpointTransfer>,
    right: Option<&EndpointTransfer>,
) -> Result<Transfer, PairedPlanError> {
    let left = left.cloned().unwrap_or_default();
    let right = right.cloned().unwrap_or_default();
    let (master, left_start, right_start, master_reply) = match (
        left.local,
        left.remote,
        left.reply,
        right.local,
        right.remote,
        right.reply,
    ) {
        (Some(local), None, Some(reply), None, Some(remote), None) => {
            (Side::Left, local, remote, reply)
        }
        (None, Some(remote), None, Some(local), None, Some(reply)) => {
            (Side::Right, remote, local, reply)
        }
        _ => return Err(PairedPlanError::NonComplementary { id }),
    };

    let local_start = if master == Side::Left {
        left_start
    } else {
        right_start
    };
    if master_reply.ordinal <= local_start.ordinal || master_reply.point < local_start.point {
        return Err(PairedPlanError::ReplyBeforeStart { id, side: master });
    }
    let local_action = action(local_start.event).expect("local start has an action");
    let remote_start = if master == Side::Left {
        right_start
    } else {
        left_start
    };
    let remote_action = action(remote_start.event).expect("remote start has an action");
    if local_action != remote_action {
        return Err(PairedPlanError::ActionMismatch { id });
    }
    let expected_reply = reply(master_reply.event).expect("master reply has reply data");
    let local_reply = match remote_start.event {
        ReplayGameBoyLinkEvent::RemoteMasterStart { local_reply, .. } => local_reply,
        _ => None,
    };
    if local_reply != Some(expected_reply) {
        return Err(PairedPlanError::ReplyMismatch { id });
    }
    Ok(Transfer {
        id,
        master,
        left_start,
        right_start,
        master_reply,
    })
}

fn validate_initial_master_continuation(
    left: &ReplayPlayer,
    right: &ReplayPlayer,
) -> Result<Option<InitialMasterContinuation>, PairedPlanError> {
    let left_coordinator = left.metadata().game_boy_link_coordinator_start_state;
    let right_coordinator = right.metadata().game_boy_link_coordinator_start_state;
    let (master, state, master_player, peer_player) = match (left_coordinator, right_coordinator) {
        (None, None) => return Ok(None),
        (Some(state), None) => (Side::Left, state, left, right),
        (None, Some(state)) => (Side::Right, state, right, left),
        (Some(_), Some(_)) => return Err(PairedPlanError::ConflictingInitialMasters),
    };
    let peer_completion = peer_player
        .metadata()
        .game_boy_link_start_state
        .and_then(|state| state.pending_passive_completion)
        .ok_or(PairedPlanError::InvalidInitialContinuation { side: master })?;
    if peer_completion.peer_byte != state.action.out_byte
        || peer_completion.remaining_t_cycles > state.action.clock_period_t_cycles
    {
        return Err(PairedPlanError::InvalidInitialContinuation { side: master });
    }

    let reply_event = if state.owner == ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply {
        master_player
            .metadata()
            .events
            .iter()
            .enumerate()
            .find_map(|(ordinal, event)| {
                let ReplayEvent::GameBoyLink { frame, tick, event } = event else {
                    return None;
                };
                matches!(
                    event,
                    ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. }
                        if *transfer_id == state.transfer_id
                )
                .then_some((ordinal, *frame, *tick, *event))
            })
            .map(|(ordinal, frame, tick, event)| {
                let start_tick = master_player
                    .metadata()
                    .game_boy_link_start_tick
                    .ok_or(PairedPlanError::MissingStartTick { side: master })?;
                Ok(LocatedEvent {
                    ordinal,
                    point: Point {
                        frame,
                        tick,
                        absolute_tick: start_tick
                            .checked_add(tick)
                            .ok_or(PairedPlanError::TickOverflow { side: master, tick })?,
                    },
                    event,
                })
            })
            .transpose()?
            .ok_or(PairedPlanError::InvalidInitialContinuation { side: master })?
            .into()
    } else {
        None
    };
    let expected_reply = reply_event
        .map(LocatedEvent::reply)
        .or(state.reply)
        .ok_or(PairedPlanError::InvalidInitialContinuation { side: master })?;
    if !expected_reply.passive {
        return Err(PairedPlanError::InvalidInitialContinuation { side: master });
    }
    Ok(Some(InitialMasterContinuation {
        master,
        state,
        reply_event,
    }))
}

fn validate_endpoint_ids(
    transfers: &[Transfer],
    initial_master: Option<InitialMasterContinuation>,
) -> Result<(), PairedPlanError> {
    let mut endpoints = HashMap::<Side, u8>::new();
    let mut counters = HashMap::<Side, u64>::new();
    if let Some(initial) = initial_master {
        endpoints.insert(initial.master, (initial.state.transfer_id >> 56) as u8);
        counters.insert(
            initial.master,
            initial.state.transfer_id & 0x00FF_FFFF_FFFF_FFFF,
        );
    }
    let mut local_order: Vec<_> = transfers.iter().collect();
    local_order.sort_by_key(|transfer| match transfer.master {
        Side::Left => transfer.left_start.ordinal,
        Side::Right => transfer.right_start.ordinal,
    });
    for transfer in local_order {
        let endpoint = (transfer.id >> 56) as u8;
        let counter = transfer.id & 0x00FF_FFFF_FFFF_FFFF;
        if endpoints
            .insert(transfer.master, endpoint)
            .is_some_and(|seen| seen != endpoint)
        {
            return Err(PairedPlanError::EndpointChanged {
                side: transfer.master,
            });
        }
        if counters
            .insert(transfer.master, counter)
            .is_some_and(|previous| counter <= previous)
        {
            return Err(PairedPlanError::CounterOrder {
                side: transfer.master,
            });
        }
    }
    if endpoints.contains_key(&Side::Left)
        && endpoints.get(&Side::Left) == endpoints.get(&Side::Right)
    {
        return Err(PairedPlanError::SharedEndpoint);
    }
    Ok(())
}

fn first_common_transfer_in_stream_order<'a>(
    right_events: &[ReplayEvent],
    transfers: &'a [Transfer],
) -> Option<&'a Transfer> {
    let by_id: HashMap<_, _> = transfers
        .iter()
        .map(|transfer| (transfer.id, transfer))
        .collect();
    right_events.iter().find_map(|event| {
        let ReplayEvent::GameBoyLink { event, .. } = event else {
            return None;
        };
        let id = transfer_id(*event);
        matches!(
            event,
            ReplayGameBoyLinkEvent::LocalMasterStart { .. }
                | ReplayGameBoyLinkEvent::RemoteMasterStart { .. }
        )
        .then(|| by_id.get(&id).copied())
        .flatten()
    })
}

fn build_causal_batches(transfers: &[Transfer]) -> Result<Vec<TransferBatch>, PairedPlanError> {
    let mut left: Vec<_> = transfers.iter().collect();
    left.sort_by_key(|transfer| transfer.left_start.ordinal);
    let mut right: Vec<_> = transfers.iter().collect();
    right.sort_by_key(|transfer| transfer.right_start.ordinal);

    let mut batches = Vec::new();
    let mut cursor = 0usize;
    while cursor < left.len() {
        if left[cursor].id == right[cursor].id {
            batches.push(TransferBatch {
                transfers: vec![left[cursor].clone()],
            });
            cursor += 1;
            continue;
        }
        if cursor + 1 >= left.len()
            || left[cursor].id != right[cursor + 1].id
            || left[cursor + 1].id != right[cursor].id
            || !crossed_masters(left[cursor], left[cursor + 1])
        {
            return Err(PairedPlanError::AmbiguousOrder {
                first: left[cursor].id,
                second: right[cursor].id,
            });
        }
        if transfer_reply(left[cursor]).passive || transfer_reply(left[cursor + 1]).passive {
            return Err(PairedPlanError::PassiveCrossedBatch {
                first: left[cursor].id,
                second: left[cursor + 1].id,
            });
        }
        batches.push(TransferBatch {
            transfers: vec![left[cursor].clone(), left[cursor + 1].clone()],
        });
        cursor += 2;
    }
    Ok(batches)
}

fn transfer_reply(transfer: &Transfer) -> ReplayGameBoyLinkReply {
    reply(transfer.master_reply.event).expect("validated transfer has a master reply")
}

fn crossed_masters(first: &Transfer, second: &Transfer) -> bool {
    if first.master == second.master {
        return false;
    }
    let (left_master, right_master) = if first.master == Side::Left {
        (first, second)
    } else {
        (second, first)
    };
    left_master.left_start.ordinal < right_master.left_start.ordinal
        && right_master.right_start.ordinal < left_master.right_start.ordinal
        && left_master.master_reply.ordinal > right_master.left_start.ordinal
        && right_master.master_reply.ordinal > left_master.right_start.ordinal
}

fn action(event: ReplayGameBoyLinkEvent) -> Option<ReplayGameBoyLinkAction> {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart {
            clock_period_t_cycles,
            out_byte,
            serial_generation,
            ..
        }
        | ReplayGameBoyLinkEvent::RemoteMasterStart {
            clock_period_t_cycles,
            out_byte,
            serial_generation,
            ..
        } => Some(ReplayGameBoyLinkAction {
            out_byte,
            clock_period_t_cycles,
            serial_generation,
        }),
        ReplayGameBoyLinkEvent::RemoteReply { .. } => None,
    }
}

fn reply(event: ReplayGameBoyLinkEvent) -> Option<ReplayGameBoyLinkReply> {
    match event {
        ReplayGameBoyLinkEvent::RemoteReply {
            out_byte,
            passive,
            serial_generation,
            ..
        } => Some(ReplayGameBoyLinkReply {
            out_byte,
            passive,
            serial_generation,
        }),
        _ => None,
    }
}

fn transfer_id(event: ReplayGameBoyLinkEvent) -> u64 {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart { transfer_id, .. }
        | ReplayGameBoyLinkEvent::RemoteMasterStart { transfer_id, .. }
        | ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. } => transfer_id,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PairedPlanError {
    Empty,
    MissingStartTick {
        side: Side,
    },
    TickOverflow {
        side: Side,
        tick: u64,
    },
    DuplicateRole {
        side: Side,
        id: u64,
        role: &'static str,
    },
    NonComplementary {
        id: u64,
    },
    ReplyBeforeStart {
        id: u64,
        side: Side,
    },
    ActionMismatch {
        id: u64,
    },
    ReplyMismatch {
        id: u64,
    },
    EndpointChanged {
        side: Side,
    },
    CounterOrder {
        side: Side,
    },
    SharedEndpoint,
    AmbiguousOrder {
        first: u64,
        second: u64,
    },
    PassiveCrossedBatch {
        first: u64,
        second: u64,
    },
    ConflictingInitialMasters,
    InvalidInitialContinuation {
        side: Side,
    },
}

impl fmt::Display for PairedPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("paired GB replay has no semantic transfers"),
            Self::MissingStartTick { side } => {
                write!(f, "{side} GB replay has link events but no start tick")
            }
            Self::TickOverflow { side, tick } => write!(
                f,
                "{side} GB replay event tick {tick} overflows its absolute replay clock"
            ),
            Self::DuplicateRole { side, id, role } => write!(
                f,
                "{side} GB transfer {id:#018X} has duplicate {role} events"
            ),
            Self::NonComplementary { id } => write!(
                f,
                "GB transfer {id:#018X} does not have complementary endpoint roles"
            ),
            Self::ReplyBeforeStart { id, side } => write!(
                f,
                "{side} GB transfer {id:#018X} reply precedes its local start"
            ),
            Self::ActionMismatch { id } => write!(
                f,
                "GB transfer {id:#018X} records different actions at its endpoints"
            ),
            Self::ReplyMismatch { id } => write!(
                f,
                "GB transfer {id:#018X} records different replies at its endpoints"
            ),
            Self::EndpointChanged { side } => {
                write!(f, "{side} GB replay changes transfer endpoint ID")
            }
            Self::CounterOrder { side } => {
                write!(f, "{side} GB replay transfer counters are not increasing")
            }
            Self::SharedEndpoint => {
                f.write_str("paired GB replay endpoints use the same transfer endpoint ID")
            }
            Self::AmbiguousOrder { first, second } => write!(
                f,
                "GB transfers {first:#018X} and {second:#018X} have an ambiguous causal order"
            ),
            Self::PassiveCrossedBatch { first, second } => write!(
                f,
                "crossed GB transfers {first:#018X} and {second:#018X} require non-passive replies"
            ),
            Self::ConflictingInitialMasters => {
                f.write_str("both paired GB replay endpoints own an in-flight master transfer")
            }
            Self::InvalidInitialContinuation { side } => write!(
                f,
                "{side} GB replay master continuation has no matching passive peer state"
            ),
        }
    }
}

impl std::error::Error for PairedPlanError {}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT_ID: u64 = 0x0100_0000_0000_0000;
    const RIGHT_ID: u64 = 0x0200_0000_0000_0000;

    fn local(id: u64, frame: u64, tick: u64, byte: u8) -> ReplayEvent {
        ReplayEvent::GameBoyLink {
            frame,
            tick,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: id,
                clock_period_t_cycles: 4096,
                out_byte: byte,
                serial_generation: 7,
            },
        }
    }

    fn remote(id: u64, frame: u64, tick: u64, byte: u8) -> ReplayEvent {
        remote_with_passive(id, frame, tick, byte, true)
    }

    fn remote_with_passive(id: u64, frame: u64, tick: u64, byte: u8, passive: bool) -> ReplayEvent {
        ReplayEvent::GameBoyLink {
            frame,
            tick,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: id,
                clock_period_t_cycles: 4096,
                out_byte: byte,
                serial_generation: 7,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: 0x34,
                    passive,
                    serial_generation: 9,
                }),
            },
        }
    }

    fn response(id: u64, frame: u64, tick: u64) -> ReplayEvent {
        response_with_passive(id, frame, tick, 0x34, true)
    }

    fn response_byte(id: u64, frame: u64, tick: u64, out_byte: u8) -> ReplayEvent {
        response_with_passive(id, frame, tick, out_byte, true)
    }

    fn response_with_passive(
        id: u64,
        frame: u64,
        tick: u64,
        out_byte: u8,
        passive: bool,
    ) -> ReplayEvent {
        ReplayEvent::GameBoyLink {
            frame,
            tick,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: id,
                out_byte,
                passive,
                serial_generation: 9,
            },
        }
    }

    fn planned_ids(plan: &PairedTransferPlan) -> Vec<Vec<u64>> {
        plan.batches
            .iter()
            .map(|batch| batch.transfers.iter().map(|transfer| transfer.id).collect())
            .collect()
    }

    #[test]
    fn stream_order_ignores_non_comparable_frame_numbers() {
        let plan = PairedTransferPlan::build(
            &[
                local(LEFT_ID, 90, 100_000, 0x12),
                response(LEFT_ID, 91, 104_096),
            ],
            Some(10),
            &[remote(LEFT_ID, 2, 800, 0x12)],
            Some(20),
        )
        .unwrap();
        assert_eq!(plan.anchor_id, LEFT_ID);
        assert_eq!(
            (plan.left_anchor_tick, plan.right_anchor_tick),
            (100_010, 820)
        );
        assert_eq!(planned_ids(&plan), vec![vec![LEFT_ID]]);
    }

    #[test]
    fn crossed_local_masters_form_one_atomic_batch() {
        let plan = PairedTransferPlan::build(
            &[
                local(LEFT_ID, 1, 100, 0x12),
                remote_with_passive(RIGHT_ID, 1, 104, 0x56, false),
                response_with_passive(LEFT_ID, 1, 200, 0x34, false),
            ],
            Some(1),
            &[
                local(RIGHT_ID, 50, 900, 0x56),
                remote_with_passive(LEFT_ID, 50, 904, 0x12, false),
                response_with_passive(RIGHT_ID, 50, 1_000, 0x34, false),
            ],
            Some(2),
        )
        .unwrap();
        assert_eq!(planned_ids(&plan), vec![vec![LEFT_ID, RIGHT_ID]]);
        assert_eq!((plan.left_anchor_tick, plan.right_anchor_tick), (101, 902));
    }

    #[test]
    fn crossed_batch_rejects_passive_reply_snapshots() {
        assert!(matches!(
            PairedTransferPlan::build(
                &[
                    local(LEFT_ID, 1, 100, 0x12),
                    remote(RIGHT_ID, 1, 104, 0x56),
                    response(LEFT_ID, 1, 200),
                ],
                Some(1),
                &[
                    local(RIGHT_ID, 50, 900, 0x56),
                    remote(LEFT_ID, 50, 904, 0x12),
                    response(RIGHT_ID, 50, 1_000),
                ],
                Some(2),
            ),
            Err(PairedPlanError::PassiveCrossedBatch { .. })
        ));
    }

    #[test]
    fn rejects_empty_missing_ticks_overflow_and_same_roles() {
        assert_eq!(
            PairedTransferPlan::build(&[], Some(1), &[], Some(2)).unwrap_err(),
            PairedPlanError::Empty
        );
        assert!(matches!(
            PairedTransferPlan::build(
                &[local(LEFT_ID, 0, 1, 1), response(LEFT_ID, 0, 2)],
                None,
                &[remote(LEFT_ID, 0, 1, 1)],
                Some(2)
            ),
            Err(PairedPlanError::MissingStartTick { side: Side::Left })
        ));
        assert!(matches!(
            PairedTransferPlan::build(
                &[local(LEFT_ID, 0, 1, 1), response(LEFT_ID, 0, 2)],
                Some(u64::MAX),
                &[remote(LEFT_ID, 0, 1, 1)],
                Some(2)
            ),
            Err(PairedPlanError::TickOverflow { .. })
        ));
        assert!(matches!(
            PairedTransferPlan::build(
                &[local(LEFT_ID, 0, 1, 1), response(LEFT_ID, 0, 2)],
                Some(1),
                &[local(LEFT_ID, 0, 1, 1)],
                Some(2)
            ),
            Err(PairedPlanError::NonComplementary { .. })
        ));
    }

    #[test]
    fn rejects_action_and_reply_data_mismatches() {
        assert!(matches!(
            PairedTransferPlan::build(
                &[local(LEFT_ID, 0, 1, 0x12), response(LEFT_ID, 0, 2)],
                Some(1),
                &[remote(LEFT_ID, 0, 1, 0x13)],
                Some(2)
            ),
            Err(PairedPlanError::ActionMismatch { .. })
        ));
        assert!(matches!(
            PairedTransferPlan::build(
                &[
                    local(LEFT_ID, 0, 1, 0x12),
                    response_byte(LEFT_ID, 0, 2, 0x35)
                ],
                Some(1),
                &[remote(LEFT_ID, 0, 1, 0x12)],
                Some(2)
            ),
            Err(PairedPlanError::ReplyMismatch { .. })
        ));
    }

    #[test]
    fn same_tick_metadata_order_remains_causal() {
        let second = LEFT_ID + 1;
        let plan = PairedTransferPlan::build(
            &[
                local(LEFT_ID, 0, 10, 0x12),
                response(LEFT_ID, 0, 10),
                local(second, 0, 10, 0x56),
                response(second, 0, 10),
            ],
            Some(100),
            &[remote(LEFT_ID, 99, 500, 0x12), remote(second, 1, 500, 0x56)],
            Some(200),
        )
        .unwrap();
        assert_eq!(planned_ids(&plan), vec![vec![LEFT_ID], vec![second]]);
    }

    #[test]
    fn long_matching_stream_uses_the_linear_zipper_order() {
        const TRANSFERS: u64 = 2_048;
        let mut left = Vec::with_capacity((TRANSFERS * 2) as usize);
        let mut right = Vec::with_capacity(TRANSFERS as usize);
        for counter in 0..TRANSFERS {
            let id = LEFT_ID + counter;
            let tick = counter * 10;
            left.push(local(id, counter, tick, counter as u8));
            left.push(response(id, counter, tick + 1));
            right.push(remote(id, TRANSFERS - counter, tick + 2, counter as u8));
        }
        let plan = PairedTransferPlan::build(&left, Some(1), &right, Some(2)).unwrap();
        assert_eq!(plan.batches.len(), TRANSFERS as usize);
        assert!(plan.batches.iter().all(|batch| batch.transfers.len() == 1));
    }
}
