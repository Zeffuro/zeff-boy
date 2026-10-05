use super::*;
use crate::{CHANNEL_PROTOCOL, CONTROL_CHANNEL_ID, CONTROL_LABEL, INPUT_CHANNEL_ID, INPUT_LABEL};

fn server() -> IceServer {
    IceServer {
        urls: vec!["stun:example.com:3478".into()],
        username: None,
        credential: None,
    }
}

#[test]
fn server_url_and_credential_boundaries_are_shared() {
    assert!(validate_ice_servers(&[], false).is_ok());
    assert!(validate_ice_servers(&vec![server(); MAX_ICE_SERVERS], false).is_ok());
    assert_eq!(
        validate_ice_servers(&vec![server(); MAX_ICE_SERVERS + 1], false),
        Err(IceConfigError::Servers)
    );
    let mut ice = server();
    ice.urls = vec!["stun:example.com".into(); MAX_ICE_URLS];
    ice.username = Some("u".repeat(MAX_ICE_USERNAME_BYTES));
    ice.credential = Some("p".repeat(MAX_ICE_CREDENTIAL_BYTES));
    assert!(validate_ice_servers(&[ice.clone()], false).is_ok());
    ice.urls.push("stun:other.example".into());
    assert_eq!(
        validate_ice_servers(&[ice.clone()], false),
        Err(IceConfigError::Urls)
    );
    ice.urls.clear();
    assert_eq!(
        validate_ice_servers(&[ice.clone()], false),
        Err(IceConfigError::Urls)
    );
    ice.urls = vec![format!("stun:{}", "a".repeat(MAX_ICE_URL_BYTES - 5))];
    assert!(validate_ice_servers(&[ice.clone()], false).is_ok());
    ice.urls[0].push('a');
    assert_eq!(
        validate_ice_servers(&[ice.clone()], false),
        Err(IceConfigError::Urls)
    );
    ice.urls = server().urls;
    ice.username.as_mut().unwrap().push('u');
    assert_eq!(
        validate_ice_servers(&[ice.clone()], false),
        Err(IceConfigError::Credentials)
    );
    ice.username = None;
    ice.credential.as_mut().unwrap().push('p');
    assert_eq!(
        validate_ice_servers(&[ice], false),
        Err(IceConfigError::Credentials)
    );
}

#[test]
fn credentials_are_bounded_in_utf8_bytes_without_exposing_them() {
    let mut ice = server();
    ice.username = Some("é".repeat(MAX_ICE_USERNAME_BYTES / 2));
    ice.credential = Some("密".repeat(MAX_ICE_CREDENTIAL_BYTES / 3));
    assert!(validate_ice_servers(&[ice.clone()], false).is_ok());
    ice.username.as_mut().unwrap().push('é');
    let error = validate_ice_servers(&[ice.clone()], false).unwrap_err();
    assert!(!error.to_string().contains("密"));
    ice.username = None;
    ice.credential.as_mut().unwrap().push('密');
    assert_eq!(
        validate_ice_servers(&[ice], false),
        Err(IceConfigError::Credentials)
    );
}

#[test]
fn relay_and_url_syntax_policy_is_identical_for_all_consumers() {
    for url in [
        "stun:example.com",
        "stuns:example.com:5349",
        "stun:[::1]:3478",
    ] {
        assert!(valid_ice_url(url, false));
    }
    for url in ["turn:example.com?transport=udp", "turns:example.com:5349"] {
        let mut ice = server();
        ice.urls = vec![url.into()];
        assert_eq!(
            validate_ice_servers(&[ice.clone()], false),
            Err(IceConfigError::Urls)
        );
        assert!(validate_ice_servers(&[ice], true).is_ok());
    }
    for url in [
        "",
        "stun:",
        "turn:",
        "https://example.com",
        "STUN:example.com",
        "stun:user@host",
        "stun:host\0",
        "stun:host\n",
        "stun: host",
        "stun:host\t",
    ] {
        assert!(!valid_ice_url(url, true), "{url:?}");
    }
}

#[test]
fn aggregate_limit_includes_json_escaping_and_credentials() {
    let mut ice = server();
    ice.urls = vec![format!("stun:{}", "a".repeat(MAX_ICE_URL_BYTES - 5)); MAX_ICE_URLS];
    let oversized = vec![ice; MAX_ICE_SERVERS];
    assert!(serde_json::to_vec(&oversized).unwrap().len() > MAX_ICE_CONFIG_BYTES);
    assert_eq!(
        validate_ice_servers(&oversized, false),
        Err(IceConfigError::Size)
    );
    let mut escaped = server();
    escaped.username = Some("\u{1}".repeat(MAX_ICE_USERNAME_BYTES));
    escaped.credential = Some("\u{2}".repeat(MAX_ICE_CREDENTIAL_BYTES));
    let escaped = vec![escaped; MAX_ICE_SERVERS];
    assert!(serde_json::to_vec(&escaped).unwrap().len() > MAX_ICE_CONFIG_BYTES);
    assert_eq!(
        validate_ice_servers(&escaped, false),
        Err(IceConfigError::Size)
    );
}

#[test]
fn exact_json_budget_and_channel_contract_are_stable() {
    use io::Write;
    let mut budget = JsonBudget(0);
    budget.write_all(&vec![b' '; MAX_ICE_CONFIG_BYTES]).unwrap();
    assert!(budget.write_all(b"x").is_err());
    assert_eq!(budget.0, MAX_ICE_CONFIG_BYTES);
    assert_eq!((CONTROL_CHANNEL_ID, INPUT_CHANNEL_ID), (0, 1));
    assert_eq!(
        (CONTROL_LABEL, INPUT_LABEL, CHANNEL_PROTOCOL),
        ("zeff-control", "zeff-input", "zeff-netplay-v1")
    );
}
