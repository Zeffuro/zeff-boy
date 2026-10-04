use super::*;

fn row(adapter: &str, address: &str, recommended: bool) -> HostAddress {
    HostAddress {
        adapter: adapter.into(),
        address: address.parse().unwrap(),
        recommended,
    }
}

fn snapshot() -> Vec<HostAddress> {
    vec![
        row("Ethernet", "192.168.1.10", true),
        row("Wi-Fi", "192.168.1.20", false),
        row("Loopback", "127.0.0.1", false),
    ]
}

#[test]
fn first_snapshot_selects_recommendation_but_preserves_existing_custom_ip() {
    let mut picker = HostAddresses::new(false);
    let mut address = String::new();
    picker.supply(snapshot(), &mut address);
    assert_eq!(address, "192.168.1.10");
    assert_eq!(picker.selected.as_ref().unwrap().adapter, "Ethernet");
    let mut picker = HostAddresses::new(false);
    let mut address = "fd00::1".into();
    picker.supply(snapshot(), &mut address);
    assert!(picker.custom);
    assert_eq!(address, "fd00::1");
}

#[test]
fn forwarded_route_recommends_only_loopback() {
    let mut picker = HostAddresses::new(true);
    let mut address = String::new();
    picker.supply(snapshot(), &mut address);
    assert_eq!(address, "127.0.0.1");
    assert_eq!(
        picker.selected.as_ref().unwrap().adapter,
        "Forwarded connection"
    );
    assert_eq!(
        picker.choices.iter().filter(|row| row.recommended).count(),
        1
    );
}

#[test]
fn forwarded_choice_made_before_discovery_stays_available() {
    let mut picker = HostAddresses::new(true);
    picker.selected = Some(picker.choices[0].clone());
    let mut address = "127.0.0.1".into();
    picker.supply(snapshot(), &mut address);
    assert_eq!(address, "127.0.0.1");
    assert!(picker.validate(&address).is_ok());
    assert_eq!(
        picker.selected.as_ref().unwrap().adapter,
        "Forwarded connection"
    );
}

#[test]
fn refresh_keeps_user_choice_and_blocks_a_removed_adapter() {
    let mut picker = HostAddresses::new(false);
    let mut address = "192.168.1.20".into();
    picker.supply(snapshot(), &mut address);
    picker.supply(vec![row("Ethernet", "192.168.1.10", true)], &mut address);
    assert_eq!(address, "192.168.1.20");
    assert!(picker.validate(&address).is_err());
    picker.supply(snapshot(), &mut address);
    assert!(picker.validate(&address).is_ok());
    assert_eq!(address, "192.168.1.20");
}

#[test]
fn adapter_name_and_address_both_identify_the_selection() {
    let mut picker = HostAddresses::new(false);
    let mut address = "192.168.1.10".into();
    picker.supply(snapshot(), &mut address);
    picker.supply(vec![row("VPN", "192.168.1.10", true)], &mut address);
    assert!(picker.validate(&address).is_err());
}

#[test]
fn async_snapshot_does_not_override_a_choice_made_while_loading() {
    let mut picker = HostAddresses::new(false);
    let (sender, receiver) = mpsc::channel();
    picker.pending = Some(receiver);
    let mut address = "127.0.0.1".into();
    picker.selected = Some(row("Loopback", "127.0.0.1", false));
    picker.poll(&mut address);
    assert!(picker.pending.is_some());
    sender.send(snapshot()).unwrap();
    picker.poll(&mut address);
    assert!(picker.pending.is_none());
    assert_eq!(address, "127.0.0.1");
    assert!(!picker.failed);
}

#[test]
fn custom_choice_while_loading_survives_and_failed_worker_is_retryable() {
    let mut picker = HostAddresses::new(false);
    let (sender, receiver) = mpsc::channel();
    picker.pending = Some(receiver);
    picker.custom = true;
    let mut address = "192.168.99.5".into();
    sender.send(snapshot()).unwrap();
    picker.poll(&mut address);
    assert_eq!(address, "192.168.99.5");
    assert!(picker.selected.is_none());
    let (sender, receiver) = mpsc::channel();
    picker.pending = Some(receiver);
    drop(sender);
    picker.poll(&mut address);
    assert!(picker.failed && picker.ready && picker.pending.is_none());
    picker.supply(snapshot(), &mut address);
    assert!(!picker.failed);
    assert_eq!(address, "192.168.99.5");
}

#[test]
fn empty_lan_snapshot_leaves_loopback_available_without_recommending_it() {
    let mut picker = HostAddresses::new(false);
    let mut address = String::new();
    picker.supply(vec![row("Loopback", "127.0.0.1", false)], &mut address);
    assert!(address.is_empty() && picker.selected.is_none());
    assert_eq!(picker.choices.len(), 1);
}
