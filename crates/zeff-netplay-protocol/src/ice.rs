use std::{fmt, io};

use crate::IceServer;

pub const MAX_ICE_SERVERS: usize = 8;
pub const MAX_ICE_URLS: usize = 4;
pub const MAX_ICE_URL_BYTES: usize = 256;
pub const MAX_ICE_USERNAME_BYTES: usize = 128;
pub const MAX_ICE_CREDENTIAL_BYTES: usize = 256;
pub const MAX_ICE_CONFIG_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IceConfigError {
    Servers,
    Urls,
    Credentials,
    Size,
}

impl fmt::Display for IceConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Servers => "too many ICE servers",
            Self::Urls => "ICE URLs violate connection policy",
            Self::Credentials => "ICE credentials exceed limit",
            Self::Size => "ICE configuration exceeds limit",
        })
    }
}

impl std::error::Error for IceConfigError {}

pub fn valid_ice_url(url: &str, relay_allowed: bool) -> bool {
    let Some((scheme, authority)) = url.split_once(':') else {
        return false;
    };
    url.len() <= MAX_ICE_URL_BYTES
        && !authority.is_empty()
        && (matches!(scheme, "stun" | "stuns")
            || relay_allowed && matches!(scheme, "turn" | "turns"))
        && !url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control() || b == b'@')
}

pub fn validate_ice_servers(
    servers: &[IceServer],
    relay_allowed: bool,
) -> Result<(), IceConfigError> {
    if servers.len() > MAX_ICE_SERVERS {
        return Err(IceConfigError::Servers);
    }
    for server in servers {
        if server.urls.is_empty()
            || server.urls.len() > MAX_ICE_URLS
            || server
                .urls
                .iter()
                .any(|url| !valid_ice_url(url, relay_allowed))
        {
            return Err(IceConfigError::Urls);
        }
        if server
            .username
            .as_ref()
            .is_some_and(|v| v.len() > MAX_ICE_USERNAME_BYTES)
            || server
                .credential
                .as_ref()
                .is_some_and(|v| v.len() > MAX_ICE_CREDENTIAL_BYTES)
        {
            return Err(IceConfigError::Credentials);
        }
    }
    serde_json::to_writer(JsonBudget(0), servers).map_err(|_| IceConfigError::Size)
}

struct JsonBudget(usize);

impl io::Write for JsonBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_ICE_CONFIG_BYTES - self.0;
        if bytes.len() > remaining {
            return Err(io::Error::other("ICE configuration exceeds limit"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
