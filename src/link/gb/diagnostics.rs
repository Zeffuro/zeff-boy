use zeff_emu_common::replay::ReplayGameBoyLinkReply;
use zeff_gb_core::hardware::bus::GameBoyLinkReply;

pub(super) fn format_reply(reply: GameBoyLinkReply) -> String {
    format!(
        "out={:02X} passive={} gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}

pub(super) fn format_replay_reply(reply: ReplayGameBoyLinkReply) -> String {
    format!(
        "out={:02X} passive={} gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
    )
}
