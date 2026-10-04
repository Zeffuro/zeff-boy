use super::*;

pub struct PacketCodec {
    player: Player,
    secret: [u8; 32],
    transcript: [u8; 32],
    sent: u64,
    received: u64,
    sent_close: bool,
    received_close: bool,
    terminal: bool,
}

impl PacketCodec {
    pub(super) fn new(player: Player, secret: [u8; 32], transcript: [u8; 32]) -> Self {
        Self {
            player,
            secret,
            transcript,
            sent: 0,
            received: 0,
            sent_close: false,
            received_close: false,
            terminal: false,
        }
    }

    pub fn transcript(&self) -> [u8; 32] {
        self.transcript
    }

    pub fn encode(&mut self, message: &Message) -> Result<Vec<u8>> {
        let result = self.encode_inner(message);
        if result.is_err() {
            self.terminal = true;
        }
        result
    }

    fn encode_inner(&mut self, message: &Message) -> Result<Vec<u8>> {
        ensure!(!self.terminal && !self.sent_close, "connection is terminal");
        ensure!(
            !self.received_close || matches!(message, Message::Close { .. }),
            "only Close acknowledgment permitted"
        );
        if let Message::Input { player, .. } = message {
            ensure!(
                *player == self.player,
                "input player does not own this port"
            );
        }
        if let Message::Progress { frame, confirmed } = message {
            ensure!(
                confirmed <= frame,
                "confirmed progress exceeds simulated frame"
            );
        }
        if let Message::PauseChange { request, .. } | Message::PauseAck { request } = message {
            ensure!(*request != 0, "invalid pause request ID");
        }
        if let Message::Chat { text } = message {
            validate_chat(text)?;
        }
        let next = self
            .sent
            .checked_add(1)
            .context("send sequence exhausted")?;
        let mut packet = Vec::with_capacity(HEADER_LEN + 136 + TAG_LEN);
        packet.extend_from_slice(MAGIC);
        packet.extend_from_slice(&VERSION.to_be_bytes());
        packet.extend_from_slice(&self.transcript);
        packet.push(role(self.player));
        packet.extend_from_slice(&self.sent.to_be_bytes());
        messages::encode(&mut packet, message);
        ensure!(
            packet.len() + TAG_LEN <= MAX_PACKET,
            "invalid packet length"
        );
        let signature = tag(&self.secret, &[PACKET_DOMAIN, &packet]);
        packet.extend_from_slice(&signature);
        self.sent = next;
        self.sent_close = matches!(message, Message::Close { .. });
        Ok(packet)
    }

    pub fn decode(&mut self, packet: &[u8]) -> Result<Message> {
        let result = self.decode_inner(packet);
        if result.is_err() {
            self.terminal = true;
        }
        result
    }

    fn decode_inner(&mut self, packet: &[u8]) -> Result<Message> {
        ensure!(
            !self.terminal && !self.received_close,
            "connection is terminal"
        );
        ensure!(
            (HEADER_LEN + TAG_LEN..=MAX_PACKET).contains(&packet.len()),
            "invalid packet length"
        );
        let (body, signature) = packet.split_at(packet.len() - TAG_LEN);
        ensure!(
            &body[..4] == MAGIC && body[4..6] == VERSION.to_be_bytes(),
            "invalid packet version"
        );
        ensure!(body[6..38] == self.transcript, "packet session mismatch");
        ensure!(
            body[38] == role(other(self.player)),
            "packet sender role mismatch"
        );
        ensure!(
            u64::from_be_bytes(body[39..47].try_into()?) == self.received,
            "packet sequence mismatch"
        );
        verify(&self.secret, &[PACKET_DOMAIN, body], signature)?;
        let message = messages::decode(body[47], &body[HEADER_LEN..], other(self.player))?;
        ensure!(
            !self.sent_close || matches!(message, Message::Close { .. }),
            "gameplay packet after local Close"
        );
        self.received = self
            .received
            .checked_add(1)
            .context("receive sequence exhausted")?;
        self.received_close = matches!(message, Message::Close { .. });
        Ok(message)
    }

    pub fn authenticate_datagram(&self, body: &[u8]) -> [u8; 32] {
        tag(
            &self.secret,
            &[b"zeff-netplay-input-v1", &self.transcript, body],
        )
    }

    pub fn verify_datagram(&self, body: &[u8], signature: &[u8]) -> Result<()> {
        verify(
            &self.secret,
            &[b"zeff-netplay-input-v1", &self.transcript, body],
            signature,
        )
    }
}
