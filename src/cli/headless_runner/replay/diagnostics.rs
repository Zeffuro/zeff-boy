use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayPlayer};

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn compare_game_boy_link_event_prefix(
    label: &str,
    player: &ReplayPlayer,
    generated_events: &[ReplayEvent],
) -> String {
    let recorded_events = recorded_game_boy_link_events(player);
    let compare_len = recorded_events.len().min(generated_events.len());
    if let Some(index) = (0..compare_len).find(|&index| {
        game_boy_link_event_signature(&recorded_events[index])
            != game_boy_link_event_signature(&generated_events[index])
    }) {
        return format!(
            "{label}_first_link_event_mismatch index={} recorded={} generated={}",
            index,
            format_game_boy_link_event_for_diagnostic(&recorded_events[index]),
            format_game_boy_link_event_for_diagnostic(&generated_events[index])
        );
    }

    if generated_events.len() != recorded_events.len() {
        let next_recorded = recorded_events
            .get(compare_len)
            .map(format_game_boy_link_event_for_diagnostic)
            .unwrap_or_else(|| "none".to_string());
        let next_generated = generated_events
            .get(compare_len)
            .map(format_game_boy_link_event_for_diagnostic)
            .unwrap_or_else(|| "none".to_string());
        return format!(
            "{label}_link_event_prefix_match generated={} recorded={} next_recorded={} next_generated={}",
            generated_events.len(),
            recorded_events.len(),
            next_recorded,
            next_generated
        );
    }

    format!("{label}_link_events_match count={}", generated_events.len())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn recorded_game_boy_link_events(player: &ReplayPlayer) -> Vec<ReplayEvent> {
    player
        .metadata()
        .events
        .iter()
        .filter(|event| matches!(event, ReplayEvent::GameBoyLink { .. }))
        .cloned()
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn game_boy_link_event_signature(event: &ReplayEvent) -> String {
    let ReplayEvent::GameBoyLink { event, .. } = event else {
        return "not_gb_link".to_string();
    };
    normalized_game_boy_link_event_signature(*event)
}

#[cfg(not(target_arch = "wasm32"))]
fn format_game_boy_link_event_for_diagnostic(event: &ReplayEvent) -> String {
    let ReplayEvent::GameBoyLink { frame, tick, event } = event else {
        return "not_gb_link".to_string();
    };
    format!(
        "frame={} tick={} {}",
        frame,
        tick,
        normalized_game_boy_link_event_signature(*event)
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn normalized_game_boy_link_event_signature(event: ReplayGameBoyLinkEvent) -> String {
    match event {
        ReplayGameBoyLinkEvent::LocalMasterStart {
            transfer_id,
            clock_period_t_cycles,
            out_byte,
            serial_generation,
        } => format!(
            "local_master transfer={} out={:02X} period={} gen={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            clock_period_t_cycles,
            serial_generation
        ),
        ReplayGameBoyLinkEvent::RemoteMasterStart {
            transfer_id,
            clock_period_t_cycles,
            out_byte,
            serial_generation,
            local_reply,
        } => format!(
            "remote_master transfer={} out={:02X} period={} gen={} reply={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            clock_period_t_cycles,
            serial_generation,
            local_reply
                .map(format_game_boy_link_reply_for_diagnostic)
                .unwrap_or_else(|| "none".to_string())
        ),
        ReplayGameBoyLinkEvent::RemoteReply {
            transfer_id,
            out_byte,
            passive,
            serial_generation,
        } => format!(
            "remote_reply transfer={} out={:02X} passive={} gen={}",
            normalized_game_boy_transfer_id(transfer_id),
            out_byte,
            passive,
            serial_generation
        ),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn normalized_game_boy_transfer_id(transfer_id: u64) -> String {
    let endpoint = transfer_id >> 56;
    let counter = transfer_id & 0x00FF_FFFF_FFFF_FFFF;
    format!("ep{endpoint}:{}", counter)
}

#[cfg(not(target_arch = "wasm32"))]
fn format_game_boy_link_reply_for_diagnostic(
    reply: zeff_emu_common::replay::ReplayGameBoyLinkReply,
) -> String {
    format!(
        "out={:02X}/passive={}/gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}
