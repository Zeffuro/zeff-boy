use anyhow::{Context, Result, ensure};
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::protocol::valid_hex;

pub(crate) const DEFAULT_URL: &str = "wss://lobby.fakegaming.eu/v1/ws";
const INVITE_PREFIX: &str = "zeff-netplay:1/";

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) url: String,
    pub(crate) access_token: String,
    pub(crate) input_delay: InputDelay,
}

impl Options {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_url(&self.url)?;
        ensure!(self.access_token.len() <= 256, "Lobby key is too long");
        Ok(())
    }
}

pub(crate) fn validate_url(value: &str) -> Result<()> {
    ensure!(value.len() <= 512, "Lobby address is too long");
    let url = url::Url::parse(value).context("Enter a wss:// lobby address")?;
    ensure!(
        url.scheme() == "wss"
            || (url.scheme() == "ws"
                && url.host_str().is_some_and(|host| {
                    host == "localhost"
                        || host
                            .trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                })),
        "Use wss:// for a remote lobby"
    );
    ensure!(
        url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Lobby address must not contain credentials, a query or a fragment"
    );
    Ok(())
}

pub(crate) fn invitation(url: &str, room: &str, secret: [u8; 32], delay: InputDelay) -> String {
    format!(
        "{INVITE_PREFIX}{room}/{}/{}/{}",
        const_hex::encode(secret),
        delay.frames(),
        const_hex::encode(url)
    )
}

pub(crate) fn parse_invitation(
    value: &str,
    expected_url: &str,
) -> Result<(String, [u8; 32], InputDelay)> {
    ensure!(value.len() <= 1200, "Invalid lobby invitation");
    let mut fields = value
        .strip_prefix(INVITE_PREFIX)
        .context("Paste a lobby invitation")?
        .split('/');
    let room = fields.next().unwrap_or_default();
    let secret = fields.next().unwrap_or_default();
    let delay = fields.next().unwrap_or_default();
    let encoded_url = fields.next().unwrap_or_default();
    ensure!(
        fields.next().is_none() && valid_hex(room, 24) && valid_hex(secret, 64) && delay.len() == 1,
        "Invalid lobby invitation"
    );
    let url = String::from_utf8(const_hex::decode(encoded_url).context("Invalid lobby address")?)?;
    validate_url(&url)?;
    // Keep a private server key bound to the server the user selected.
    ensure!(
        url == expected_url,
        "This invitation uses a different lobby. Select that server first."
    );
    Ok((
        room.to_owned(),
        const_hex::decode_to_array(secret)?,
        InputDelay::new(delay.parse()?)?,
    ))
}
