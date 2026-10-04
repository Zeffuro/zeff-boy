use super::*;

impl Connection {
    #[cfg(any(feature = "test-support", feature = "native-proof"))]
    pub fn send_invalid_length_for_test(&mut self) -> Result<()> {
        ensure!(!self.terminal, "connection is terminal");
        let result = self.io.write(
            &mut self.stream,
            &[255; 4],
            Instant::now() + IO_BUDGET,
            self.cancellation.as_deref(),
        );
        if result.is_err() {
            self.terminate();
        }
        result
    }

    pub fn transcript(&self) -> [u8; 32] {
        self.transcript
    }

    pub fn send(&mut self, message: &Message) -> Result<()> {
        ensure!(!self.terminal, "connection is terminal");
        let result = self.send_inner(message);
        if result.is_ok() && matches!(message, Message::Close { .. }) {
            self.sent_close = true;
        }
        if result.is_err() || (self.sent_close && self.received_close) {
            self.terminate();
        }
        result
    }

    fn send_inner(&mut self, message: &Message) -> Result<()> {
        let deadline = Instant::now() + IO_BUDGET;
        ensure!(!self.sent_close, "local Close already sent");
        ensure!(
            !self.received_close || matches!(message, Message::Close { .. }),
            "only Close acknowledgment permitted"
        );
        let next = self
            .send_sequence
            .checked_add(1)
            .context("send sequence exhausted")?;
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
        let packet = self.encode(message);
        self.io.write(
            &mut self.stream,
            &(packet.len() as u32).to_be_bytes(),
            deadline,
            self.cancellation.as_deref(),
        )?;
        self.io.write(
            &mut self.stream,
            &packet,
            deadline,
            self.cancellation.as_deref(),
        )?;
        self.send_sequence = next;
        Ok(())
    }

    pub fn peer_disconnected(&mut self) -> Result<bool> {
        ensure!(!self.terminal, "connection is terminal");
        let result = self.io.peer_disconnected(&self.stream);
        if result.is_err() || matches!(result, Ok(true)) {
            self.terminate();
        }
        result
    }

    pub fn receive(&mut self) -> Result<Message> {
        ensure!(!self.terminal, "connection is terminal");
        let result = self.receive_inner();
        if matches!(result, Ok(Message::Close { .. })) {
            self.received_close = true;
        }
        if result.is_err() || (self.sent_close && self.received_close) {
            self.terminate();
        }
        result
    }

    fn receive_inner(&mut self) -> Result<Message> {
        let deadline = Instant::now() + IO_BUDGET;
        ensure!(!self.received_close, "peer Close already received");
        let next = self
            .receive_sequence
            .checked_add(1)
            .context("receive sequence exhausted")?;
        let mut length = [0; 4];
        self.io.read(
            &mut self.stream,
            &mut length,
            deadline,
            self.cancellation.as_deref(),
        )?;
        let length = u32::from_be_bytes(length) as usize;
        ensure!(
            (HEADER_LEN + TAG_LEN..=MAX_PACKET).contains(&length),
            "invalid packet length"
        );
        // A hostile length cannot allocate memory or request an unbounded read.
        let mut storage = [0; MAX_PACKET];
        let packet = &mut storage[..length];
        self.io.read(
            &mut self.stream,
            packet,
            deadline,
            self.cancellation.as_deref(),
        )?;
        let (body, received_tag) = packet.split_at(length - TAG_LEN);
        ensure!(&body[..4] == MAGIC, "invalid packet magic");
        ensure!(
            body[4..6] == VERSION.to_be_bytes(),
            "unsupported packet version"
        );
        ensure!(body[6..38] == self.transcript, "packet session mismatch");
        ensure!(
            body[38] == role(other(self.player)),
            "packet sender role mismatch"
        );
        ensure!(
            u64::from_be_bytes(body[39..47].try_into()?) == self.receive_sequence,
            "packet sequence mismatch"
        );
        verify(&self.secret, &[PACKET_DOMAIN, body], received_tag)?;
        let payload = &body[HEADER_LEN..];
        let message = messages::decode(body[47], payload, other(self.player))?;
        ensure!(
            !self.sent_close || matches!(message, Message::Close { .. }),
            "gameplay packet after local Close"
        );
        check_deadline(deadline, self.cancellation.as_deref())?;
        self.receive_sequence = next;
        Ok(message)
    }

    pub(super) fn encode(&self, message: &Message) -> Vec<u8> {
        let mut packet = Vec::with_capacity(HEADER_LEN + 136 + TAG_LEN);
        packet.extend_from_slice(MAGIC);
        packet.extend_from_slice(&VERSION.to_be_bytes());
        packet.extend_from_slice(&self.transcript);
        packet.push(role(self.player));
        packet.extend_from_slice(&self.send_sequence.to_be_bytes());
        messages::encode(&mut packet, message);
        let signature = tag(&self.secret, &[PACKET_DOMAIN, &packet]);
        packet.extend_from_slice(&signature);
        packet
    }

    pub(super) fn terminate(&mut self) {
        self.terminal = true;
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}
