use super::*;

enum Stage {
    Hello,
    Authentication,
    Ready,
    Complete,
    Failed,
}

pub struct Admission {
    player: Player,
    identity: Identity,
    secret: [u8; 32],
    local: [u8; HELLO_LEN],
    remote: [u8; HELLO_LEN],
    transcript: [u8; 32],
    stage: Stage,
}

impl Admission {
    pub fn new(player: Player, identity: Identity, secret: [u8; 32]) -> Result<Self> {
        let local = hello(player, &identity)?;
        Ok(Self {
            player,
            identity,
            secret,
            local,
            remote: [0; HELLO_LEN],
            transcript: [0; 32],
            stage: Stage::Hello,
        })
    }

    pub fn hello(&self) -> &[u8] {
        &self.local
    }

    pub fn receive(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>> {
        let result = self.receive_inner(packet);
        if result.is_err() {
            self.stage = Stage::Failed;
        }
        result
    }

    fn receive_inner(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>> {
        match self.stage {
            Stage::Hello => {
                ensure!(packet.len() == HELLO_LEN, "invalid admission hello length");
                ensure!(&packet[..4] == MAGIC, "invalid admission magic");
                ensure!(
                    packet[4..6] == VERSION.to_be_bytes(),
                    "unsupported wire version"
                );
                ensure!(
                    packet[6] == role(other(self.player)),
                    "incompatible player role"
                );
                ensure!(
                    packet[71..BUILD_INFO_OFFSET] == self.local[71..BUILD_INFO_OFFSET],
                    "session identity mismatch"
                );
                let remote_info = BuildInfo::decode(&packet[BUILD_INFO_OFFSET..])?;
                build::admit_builds(
                    &self.identity.build,
                    &self.identity.build_info,
                    &packet[39..71].try_into()?,
                    &remote_info,
                )?;
                self.remote.copy_from_slice(packet);
                let (host, guest) = self.hellos();
                self.transcript = Sha256::new()
                    .chain_update(AUTH_DOMAIN)
                    .chain_update(host)
                    .chain_update(guest)
                    .finalize()
                    .into();
                let (host, guest) = self.hellos();
                let reply = tag(
                    &self.secret,
                    &[AUTH_DOMAIN, host, guest, &[role(self.player)]],
                );
                self.stage = Stage::Authentication;
                Ok(Some(reply.to_vec()))
            }
            Stage::Authentication => {
                let (host, guest) = self.hellos();
                verify(
                    &self.secret,
                    &[AUTH_DOMAIN, host, guest, &[role(other(self.player))]],
                    packet,
                )?;
                let reply = tag(
                    &self.secret,
                    &[READY_DOMAIN, &self.transcript, &[role(self.player)]],
                );
                self.stage = Stage::Ready;
                Ok(Some(reply.to_vec()))
            }
            Stage::Ready => {
                verify(
                    &self.secret,
                    &[READY_DOMAIN, &self.transcript, &[role(other(self.player))]],
                    packet,
                )?;
                self.stage = Stage::Complete;
                Ok(None)
            }
            Stage::Complete | Stage::Failed => anyhow::bail!("admission is terminal"),
        }
    }

    fn hellos(&self) -> (&[u8], &[u8]) {
        if self.player == Player::One {
            (&self.local, &self.remote)
        } else {
            (&self.remote, &self.local)
        }
    }

    pub fn complete(&self) -> bool {
        matches!(self.stage, Stage::Complete)
    }

    pub fn into_codec(self) -> Result<PacketCodec> {
        ensure!(self.complete(), "admission is incomplete");
        Ok(PacketCodec::new(self.player, self.secret, self.transcript))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity {
            build: [1; 32],
            build_info: BuildInfo::default(),
            source: [2; 32],
            effective: [2; 32],
            media_len: 16,
            config: [3; 32],
            initial: [4; 32],
            persistent: [5; 32],
            state_format: 11,
        }
    }

    fn admit(a: &mut Admission, b: &mut Admission) -> Result<()> {
        let ah = a.hello().to_vec();
        let bh = b.hello().to_vec();
        let at = a.receive(&bh)?.unwrap();
        let bt = b.receive(&ah)?.unwrap();
        let ar = a.receive(&bt)?.unwrap();
        let br = b.receive(&at)?.unwrap();
        assert!(a.receive(&br)?.is_none());
        assert!(b.receive(&ar)?.is_none());
        Ok(())
    }

    #[test]
    fn packet_admission_preserves_secret_identity_and_ready_barrier() {
        let mut a = Admission::new(Player::One, identity(), [9; 32]).unwrap();
        let mut b = Admission::new(Player::Two, identity(), [9; 32]).unwrap();
        admit(&mut a, &mut b).unwrap();
        let mut ac = a.into_codec().unwrap();
        let mut bc = b.into_codec().unwrap();
        let message = Message::Progress {
            frame: 8,
            confirmed: 6,
        };
        let packet = ac.encode(&message).unwrap();
        assert_eq!(bc.decode(&packet).unwrap(), message);
        assert!(bc.decode(&packet).is_err());
    }

    #[test]
    fn mismatches_fail_closed_before_gameplay() {
        for kind in 0..3 {
            let mut remote = identity();
            if kind == 1 {
                remote.initial[0] ^= 1;
            }
            if kind == 2 {
                remote.build[0] ^= 1;
            }
            let mut a = Admission::new(Player::One, identity(), [9; 32]).unwrap();
            let mut b =
                Admission::new(Player::Two, remote, [if kind == 0 { 8 } else { 9 }; 32]).unwrap();
            assert!(admit(&mut a, &mut b).is_err());
            assert!(!a.complete());
            assert!(a.into_codec().is_err());
        }
    }
}
