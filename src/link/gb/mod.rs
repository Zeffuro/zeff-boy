mod diagnostics;
mod live;
mod protocol;
mod replay;
#[cfg(not(target_arch = "wasm32"))]
mod trace;

#[cfg(test)]
mod tests;

pub(crate) use live::GameBoyRemoteLink;
pub(crate) use protocol::GameBoyLinkPayloadError;
pub(crate) use replay::GameBoyReplayLink;

fn complete_passive_transfer(
    emulator: &mut zeff_gb_core::emulator::Emulator,
    peer_byte: u8,
    clock_period_t_cycles: u64,
) -> Result<(), crate::link::LinkSessionError> {
    let target = emulator.cpu_cycles().saturating_add(clock_period_t_cycles);
    if !emulator.schedule_game_boy_external_link_transfer(peer_byte, clock_period_t_cycles) {
        return Err(crate::link::LinkSessionError::MalformedPacketPayload);
    }
    while emulator.cpu_cycles() < target {
        let before = emulator.cpu_cycles();
        let (_, _, _, cycles) = emulator.step_instruction();
        if cycles == 0 || emulator.cpu_cycles() == before {
            return Err(crate::link::LinkSessionError::MalformedPacketPayload);
        }
    }
    if emulator.game_boy_link_reply_to_master_start().passive {
        return Err(crate::link::LinkSessionError::MalformedPacketPayload);
    }
    Ok(())
}
