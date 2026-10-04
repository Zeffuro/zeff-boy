use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Context, Result, bail, ensure};
use wasm_bindgen::JsValue;
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::browser::{BrowserLobby, LobbyCancellation};
use zeff_netplay_connect::protocol::{ClientMessage, SessionIdentity, VERSION};

use super::{Start, Transport};

#[path = "lobby_options.rs"]
pub(crate) mod lobby;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostOptions {
    pub(crate) address: std::net::SocketAddr,
    pub(crate) scope: ConnectionScope,
    pub(crate) input_delay: InputDelay,
}

impl HostOptions {
    pub(crate) fn validate(self) -> Result<()> {
        bail!("Use Lobby in the browser")
    }
}

pub(crate) struct Connector {
    result: Rc<RefCell<Option<Result<Start>>>>,
    invitation: Rc<RefCell<Option<String>>>,
    cancelled: Option<LobbyCancellation>,
}

fn js_error(error: JsValue) -> anyhow::Error {
    use wasm_bindgen::JsCast;
    anyhow::anyhow!(
        error
            .as_string()
            .or_else(|| error
                .dyn_ref::<js_sys::Error>()
                .and_then(|error| error.message().as_string()))
            .unwrap_or_else(|| "Lobby connection failed".into())
    )
}

pub(crate) fn executable_build() -> Result<[u8; 32]> {
    let value = js_sys::Reflect::get(&js_sys::global(), &"zeffBoyBundleIdentity".into())
        .map_err(js_error)?
        .as_string()
        .context("Reload this build to enable browser netplay")?;
    let digest = const_hex::decode_to_array(&value).context("Invalid browser build identity")?;
    ensure!(digest != [0; 32], "Invalid browser build identity");
    Ok(digest)
}

pub(crate) fn invitation_delay(_: &str, _: ConnectionScope) -> Result<InputDelay> {
    bail!("Use a Lobby invitation")
}

pub(crate) fn validate_invitation(_: &str, _: ConnectionScope) -> Result<()> {
    bail!("Use a Lobby invitation")
}

impl Connector {
    pub(crate) fn host(_: HostOptions) -> Result<(Self, String)> {
        bail!("Use Lobby in the browser")
    }
    pub(crate) fn join(_: &str, _: ConnectionScope) -> Result<Self> {
        bail!("Use Lobby in the browser")
    }

    pub(crate) fn lobby_host(options: lobby::Options, identity: SessionIdentity) -> Result<Self> {
        options.validate()?;
        let mut secret = [0; 32];
        web_sys::window()
            .context("Browser window unavailable")?
            .crypto()
            .map_err(js_error)?
            .get_random_values_with_u8_array(&mut secret)
            .map_err(js_error)?;
        let auth = ClientMessage::Create {
            version: VERSION,
            access_token: options.access_token.clone(),
            identity,
        };
        Self::spawn(options, auth, Player::One, secret)
    }

    pub(crate) fn lobby_join(
        options: lobby::Options,
        invitation: &str,
        identity: SessionIdentity,
    ) -> Result<Self> {
        options.validate()?;
        let (room, secret, delay) = lobby::parse_invitation(invitation, &options.url)?;
        ensure!(delay == options.input_delay, "Invitation delay changed");
        let auth = ClientMessage::Join {
            version: VERSION,
            access_token: options.access_token.clone(),
            room,
            identity,
        };
        Self::spawn(options, auth, Player::Two, secret)
    }

    fn spawn(
        options: lobby::Options,
        auth: ClientMessage,
        player: Player,
        secret: [u8; 32],
    ) -> Result<Self> {
        let build = executable_build()?;
        let mut socket = BrowserLobby::new(&options.url, &auth).map_err(js_error)?;
        let cancelled = socket.cancellation();
        let result = Rc::new(RefCell::new(None));
        let invitation = Rc::new(RefCell::new(None));
        let completion = result.clone();
        let published = invitation.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let established = async {
                socket.open().await.map_err(js_error)?;
                if player == Player::One {
                    *published.borrow_mut() = Some(lobby::invitation(
                        &options.url,
                        socket.room().context("Lobby did not assign a room")?,
                        secret,
                        options.input_delay,
                    ));
                }
                let peer = socket.establish().await.map_err(js_error)?;
                Ok(Start {
                    stream: Transport::Browser(peer),
                    player,
                    build,
                    secret,
                    scope: ConnectionScope::TrustedPrivate,
                    allow_different_versions: false,
                    verify_every_frame: cfg!(test),
                    input_delay: options.input_delay,
                })
            }
            .await;
            *completion.borrow_mut() = Some(established);
        });
        Ok(Self {
            result,
            invitation,
            cancelled: Some(cancelled),
        })
    }

    pub(crate) fn set_version_consent(&mut self, _: bool) {}
    pub(crate) fn take_invitation(&mut self) -> Option<String> {
        self.invitation.borrow_mut().take()
    }
    pub(crate) fn poll(&mut self) -> Result<Option<Start>> {
        let result = self.result.borrow_mut().take().transpose();
        if matches!(result, Ok(Some(_))) {
            self.cancelled.take();
        }
        result
    }
}

impl Drop for Connector {
    fn drop(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.cancel();
        }
    }
}
