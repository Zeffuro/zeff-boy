use super::*;

pub(super) fn decode(kind: u8, payload: &[u8], sender: Player) -> Result<Message> {
    let message = match kind {
        1 => {
            ensure!(payload.len() == 11, "invalid input length");
            ensure!(
                payload[0] == role(sender),
                "input player does not own peer port"
            );
            Message::Input {
                player: sender,
                frame: u64::from_be_bytes(payload[1..9].try_into()?),
                buttons: u16::from_be_bytes(payload[9..11].try_into()?),
            }
        }
        2 => {
            ensure!(payload.len() == 136, "invalid checkpoint length");
            Message::Checkpoint {
                frame: u64::from_be_bytes(payload[..8].try_into()?),
                logical: payload[8..40].try_into()?,
                video: payload[40..72].try_into()?,
                audio: payload[72..104].try_into()?,
                persistent: payload[104..136].try_into()?,
            }
        }
        3 => {
            ensure!(payload.len() == 8, "invalid close length");
            Message::Close {
                frame: u64::from_be_bytes(payload.try_into()?),
            }
        }
        4 => {
            ensure!(payload.len() == 9, "invalid pause length");
            ensure!(payload[8] <= 1, "invalid pause flag");
            Message::Pause {
                frame: u64::from_be_bytes(payload[..8].try_into()?),
                paused: payload[8] == 1,
            }
        }
        5 => {
            ensure!(payload.len() == 16, "invalid progress length");
            let frame = u64::from_be_bytes(payload[..8].try_into()?);
            let confirmed = u64::from_be_bytes(payload[8..].try_into()?);
            ensure!(
                confirmed <= frame,
                "confirmed progress exceeds simulated frame"
            );
            Message::Progress { frame, confirmed }
        }
        6 => {
            ensure!(payload.len() == 17, "invalid pause change length");
            let request = u64::from_be_bytes(payload[..8].try_into()?);
            ensure!(request != 0, "invalid pause request ID");
            ensure!(payload[16] <= 1, "invalid pause flag");
            Message::PauseChange {
                request,
                frame: u64::from_be_bytes(payload[8..16].try_into()?),
                paused: payload[16] == 1,
            }
        }
        7 => {
            ensure!(payload.len() == 8, "invalid pause acknowledgment length");
            let request = u64::from_be_bytes(payload.try_into()?);
            ensure!(request != 0, "invalid pause request ID");
            Message::PauseAck { request }
        }
        8 => {
            let text = std::str::from_utf8(payload).context("invalid chat UTF-8")?;
            validate_chat(text)?;
            Message::Chat {
                text: text.to_owned(),
            }
        }
        _ => anyhow::bail!("unknown packet kind"),
    };
    Ok(message)
}

pub(super) fn encode(packet: &mut Vec<u8>, message: &Message) {
    match message {
        Message::Chat { text } => {
            packet.push(8);
            packet.extend_from_slice(text.as_bytes());
        }
        Message::Input {
            player,
            frame,
            buttons,
        } => {
            packet.push(1);
            packet.push(role(*player));
            packet.extend_from_slice(&frame.to_be_bytes());
            packet.extend_from_slice(&buttons.to_be_bytes());
        }
        Message::Checkpoint {
            frame,
            logical,
            video,
            audio,
            persistent,
        } => {
            packet.push(2);
            packet.extend_from_slice(&frame.to_be_bytes());
            for digest in [logical, video, audio, persistent] {
                packet.extend_from_slice(digest);
            }
        }
        Message::Close { frame } => {
            packet.push(3);
            packet.extend_from_slice(&frame.to_be_bytes());
        }
        Message::Pause { frame, paused } => {
            packet.push(4);
            packet.extend_from_slice(&frame.to_be_bytes());
            packet.push(u8::from(*paused));
        }
        Message::Progress { frame, confirmed } => {
            packet.push(5);
            packet.extend_from_slice(&frame.to_be_bytes());
            packet.extend_from_slice(&confirmed.to_be_bytes());
        }
        Message::PauseChange {
            request,
            frame,
            paused,
        } => {
            packet.push(6);
            packet.extend_from_slice(&request.to_be_bytes());
            packet.extend_from_slice(&frame.to_be_bytes());
            packet.push(u8::from(*paused));
        }
        Message::PauseAck { request } => {
            packet.push(7);
            packet.extend_from_slice(&request.to_be_bytes());
        }
    }
}
