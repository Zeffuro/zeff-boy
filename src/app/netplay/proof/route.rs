use super::*;
use crate::netplay::connect::lobby;

pub(super) enum Route {
    Lan,
    Lobby(lobby::Options),
}

impl Route {
    pub(super) fn parse(
        delay: zeff_netplay::rollback::InputDelay,
        lookup: impl Fn(&str) -> Result<Option<String>>,
    ) -> Result<Self> {
        match lookup("ZEFF_NETPLAY_APP_ROUTE")?.as_deref() {
            None | Some("lan") => Ok(Self::Lan),
            Some("lobby") => {
                let options = lobby::Options {
                    url: lookup("ZEFF_NETPLAY_APP_LOBBY_URL")?
                        .context("set ZEFF_NETPLAY_APP_LOBBY_URL")?,
                    access_token: lookup("ZEFF_NETPLAY_APP_LOBBY_TOKEN")?
                        .context("set ZEFF_NETPLAY_APP_LOBBY_TOKEN, empty for a public lobby")?,
                    input_delay: delay,
                };
                options.validate()?;
                Ok(Self::Lobby(options))
            }
            Some(_) => bail!("ZEFF_NETPLAY_APP_ROUTE must be lan or lobby"),
        }
    }

    pub(super) fn is_lobby(&self) -> bool {
        matches!(self, Self::Lobby(_))
    }

    pub(super) fn validate_invitation(&self, invitation: &str) -> Result<()> {
        match self {
            Self::Lan => crate::netplay::connect::validate_invitation(
                invitation,
                ConnectionScope::TrustedPrivate,
            ),
            Self::Lobby(options) => {
                let (_, _, delay) = lobby::parse_invitation(invitation, &options.url)?;
                ensure!(
                    delay == options.input_delay,
                    "proof invitation delay differs"
                );
                Ok(())
            }
        }
    }

    pub(super) fn configure(&self, app: &mut App) {
        app.debug_windows.netplay.lobby = self.is_lobby();
        if let Self::Lobby(options) = self {
            app.debug_windows.netplay.lobby_url = options.url.clone();
            app.debug_windows.netplay.lobby_key = options.access_token.clone();
        }
    }

    pub(super) fn endpoints(
        &self,
        connection: Option<(SocketAddr, SocketAddr, ConnectionScope)>,
    ) -> Result<(Option<String>, Option<String>, &'static str)> {
        match self {
            Self::Lan => {
                let (local, peer, scope) =
                    connection.context("connector returned no owned socket")?;
                ensure!(
                    scope == ConnectionScope::TrustedPrivate,
                    "wrong proof connection scope"
                );
                Ok((
                    Some(local.to_string()),
                    Some(peer.to_string()),
                    "trusted-private-plaintext",
                ))
            }
            Self::Lobby(_) => {
                ensure!(connection.is_none(), "lobby proof used a TCP socket");
                Ok((None, None, "direct-dtls-sctp"))
            }
        }
    }
}

pub(super) fn environment(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => bail!("{name} must be UTF-8"),
    }
}

#[cfg(test)]
mod tests;
