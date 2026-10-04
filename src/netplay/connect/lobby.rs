use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::LobbyConnection;
use zeff_netplay_connect::protocol::{ClientMessage, SessionIdentity, VERSION};

use super::{Connector, Start, executable_build};
use crate::netplay::{DirectPeer, Transport};

pub(crate) use super::lobby_options::*;

impl Connector {
    pub(crate) fn lobby_host(options: Options, identity: SessionIdentity) -> Result<Self> {
        options.validate()?;
        let build = executable_build()?;
        let mut secret = [0; 32];
        getrandom::fill(&mut secret).context("Creating invitation")?;
        let (sender, receiver) = super::mpsc::sync_channel(1);
        let mut connector = Self::spawn(move |cancelled| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let connection = runtime.block_on(cancellable(cancelled, async {
                let lobby = LobbyConnection::open(
                    &options.url,
                    &ClientMessage::Create {
                        version: VERSION,
                        access_token: options.access_token,
                        identity,
                    },
                )
                .await?;
                let _ = sender.try_send(invitation(
                    &options.url,
                    lobby.room(),
                    secret,
                    options.input_delay,
                ));
                lobby.establish().await
            }))?;
            Ok(make_start(
                connection,
                runtime,
                Player::One,
                build,
                secret,
                options.input_delay,
            ))
        })?;
        connector.invitation = Some(receiver);
        Ok(connector)
    }

    pub(crate) fn lobby_join(
        options: Options,
        invitation: &str,
        identity: SessionIdentity,
    ) -> Result<Self> {
        options.validate()?;
        let (room, secret, input_delay) = parse_invitation(invitation, &options.url)?;
        let build = executable_build()?;
        Self::spawn(move |cancelled| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let connection = runtime.block_on(cancellable(cancelled, async {
                LobbyConnection::open(
                    &options.url,
                    &ClientMessage::Join {
                        version: VERSION,
                        access_token: options.access_token,
                        room,
                        identity,
                    },
                )
                .await?
                .establish()
                .await
            }))?;
            Ok(make_start(
                connection,
                runtime,
                Player::Two,
                build,
                secret,
                input_delay,
            ))
        })
    }
}

async fn cancellable<T>(
    cancelled: &super::AtomicBool,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(125), operation) => result.context("Lobby connection timed out")?,
        () = async {
            while !cancelled.load(super::Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        } => anyhow::bail!("Connection canceled"),
    }
}

fn make_start(
    connection: zeff_netplay_connect::DataConnection,
    runtime: tokio::runtime::Runtime,
    player: Player,
    build: [u8; 32],
    secret: [u8; 32],
    input_delay: InputDelay,
) -> Start {
    Start {
        stream: Transport::Direct(Box::new(DirectPeer {
            connection,
            runtime,
        })),
        player,
        build,
        secret,
        scope: ConnectionScope::TrustedPrivate,
        input_delay,
        allow_different_versions: false,
        verify_every_frame: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_binds_secret_delay_and_server_without_access_key() {
        for frames in InputDelay::MIN..=InputDelay::MAX {
            let delay = InputDelay::new(frames).unwrap();
            let value = invitation(DEFAULT_URL, &"a".repeat(24), [7; 32], delay);
            assert_eq!(
                parse_invitation(&value, DEFAULT_URL).unwrap(),
                ("a".repeat(24), [7; 32], delay)
            );
            assert!(parse_invitation(&value, "wss://other.example/v1/ws").is_err());
        }
        for value in ["", "zeff-netplay:2/foo", &"a".repeat(1201)] {
            assert!(parse_invitation(value, DEFAULT_URL).is_err());
        }
    }

    #[test]
    fn remote_lobbies_require_tls_and_no_embedded_credentials() {
        for value in [
            DEFAULT_URL,
            "ws://localhost:8080/v1/ws",
            "ws://127.0.0.1:8080/v1/ws",
            "ws://[::1]:8080/v1/ws",
        ] {
            assert!(validate_url(value).is_ok(), "{value}");
        }
        for value in [
            "ws://192.168.1.2/v1/ws",
            "https://example.com",
            "wss://key@example.com",
            "wss://example.com?key=secret",
            "wss://example.com#secret",
        ] {
            assert!(validate_url(value).is_err(), "{value}");
        }
    }
}
