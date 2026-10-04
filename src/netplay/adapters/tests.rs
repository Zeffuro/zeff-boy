use super::*;
#[cfg(target_family = "unix")]
use netdev::interface::flags::{IFF_LOOPBACK, IFF_UP};
#[cfg(target_family = "windows")]
use netdev::interface::flags::{IFF_LOOPBACK as LOOPBACK, IFF_UP as UP};
use netdev::interface::ipv6_addr_flags::Ipv6AddrFlags;

#[cfg(target_family = "unix")]
const UP: u32 = IFF_UP as u32;
#[cfg(target_family = "unix")]
const LOOPBACK: u32 = IFF_LOOPBACK as u32;

fn candidate(adapter: &str, address: &str) -> Candidate {
    Candidate {
        adapter: adapter.to_owned(),
        address: address.parse().unwrap(),
        up: true,
        active: true,
        physical: false,
        default: false,
    }
}

fn lan() -> Vec<HostAddress> {
    select(vec![candidate("LAN", "192.168.1.10")])
}

fn interface() -> Interface {
    let mut interface = Interface::dummy();
    interface.name = "adapter".to_owned();
    interface.flags = UP;
    interface.oper_state = OperState::Up;
    interface.ipv4 = vec!["192.168.1.10/24".parse().unwrap()];
    interface
}

fn snapshot(interface: &Interface) -> Vec<HostAddress> {
    let mut candidates = Vec::new();
    append_candidates(interface, adapter_name(interface), false, &mut candidates);
    select(candidates)
}

#[test]
fn empty_enumeration_has_one_stable_loopback_without_recommendation() {
    assert_eq!(
        from_interfaces(Vec::new()),
        vec![HostAddress {
            adapter: "Loopback".to_owned(),
            address: "127.0.0.1".parse().unwrap(),
            recommended: false,
        }]
    );
}

#[test]
fn rejects_every_address_outside_the_bind_scope() {
    let forbidden = [
        "0.0.0.0",
        "8.8.8.8",
        "169.254.3.4",
        "224.0.0.1",
        "255.255.255.255",
        "100.63.255.255",
        "100.128.0.0",
        "::",
        "fe80::1",
        "2001:db8::1",
        "ff02::1",
        "::ffff:192.168.1.10",
    ];
    for address in forbidden {
        assert_eq!(
            select(vec![candidate("invalid", address)]),
            select(Vec::new()),
            "{address}"
        );
    }
}

#[test]
fn accepts_all_private_families_and_recommends_exactly_one() {
    let addresses = [
        "10.1.2.3",
        "172.16.2.3",
        "192.168.2.3",
        "100.64.2.3",
        "fd00::3",
    ];
    let result = select(addresses.iter().map(|ip| candidate("LAN", ip)).collect());
    assert_eq!(result.len(), addresses.len() + 1);
    assert_eq!(result.iter().filter(|row| row.recommended).count(), 1);
    assert!(result[0].recommended);
    for address in addresses {
        assert!(
            result
                .iter()
                .any(|row| row.address == address.parse::<IpAddr>().unwrap())
        );
    }
}

#[test]
fn loopback_variants_collapse_to_the_fallback() {
    assert_eq!(
        select(vec![candidate("lo", "127.0.0.2"), candidate("lo", "::1")]),
        select(Vec::new())
    );
    let mut loopback = interface();
    loopback.flags |= LOOPBACK;
    assert_eq!(snapshot(&loopback), select(Vec::new()));
}

#[test]
fn excludes_down_dormant_and_absent_adapters() {
    let mut down = candidate("down", "192.168.1.10");
    down.up = false;
    assert_eq!(select(vec![down]), select(Vec::new()));
    let mut adapter = interface();
    adapter.flags = 0;
    assert_eq!(snapshot(&adapter), select(Vec::new()));
    adapter.flags = UP;
    for state in [
        OperState::Down,
        OperState::Dormant,
        OperState::LowerLayerDown,
        OperState::NotPresent,
        OperState::Testing,
    ] {
        adapter.oper_state = state;
        assert_eq!(snapshot(&adapter), select(Vec::new()), "{state:?}");
    }
    adapter.oper_state = OperState::Unknown;
    assert_eq!(snapshot(&adapter).len(), 2);
}

#[test]
fn active_physical_private_ipv4_wins_over_default_vpn_and_ula() {
    let mut vpn = candidate("VPN", "10.9.0.1");
    vpn.default = true;
    let mut ethernet = candidate("Ethernet", "192.168.1.10");
    ethernet.physical = true;
    let mut cgnat = candidate("CGNAT", "100.100.1.1");
    cgnat.physical = true;
    cgnat.default = true;
    let mut ipv6 = candidate("IPv6", "fd00::1");
    ipv6.physical = true;
    ipv6.default = true;
    let result = select(vec![vpn, cgnat, ipv6, ethernet]);
    assert_eq!(result[0].adapter, "Ethernet");
    assert!(result[0].recommended);
}

#[test]
fn default_physical_adapter_wins_with_stable_ties() {
    let mut other = candidate("A", "192.168.2.10");
    other.physical = true;
    let mut default = candidate("Z", "192.168.1.10");
    default.physical = true;
    default.default = true;
    assert_eq!(select(vec![other, default])[0].adapter, "Z");
    let a = select(vec![candidate("Z", "10.2.0.1"), candidate("A", "10.1.0.1")]);
    let b = select(vec![candidate("A", "10.1.0.1"), candidate("Z", "10.2.0.1")]);
    assert_eq!(a, b);
    assert_eq!(a[0].adapter, "A");
}

#[test]
fn duplicate_addresses_preserve_distinct_adapters_and_all_assigned_ips() {
    let result = select(vec![
        candidate("A", "192.168.1.11"),
        candidate("A", "192.168.1.10"),
        candidate("A", "192.168.1.10"),
        candidate("B", "192.168.1.10"),
    ]);
    assert_eq!(result.len(), 4);
    assert_eq!(result[0].address, "192.168.1.10".parse::<IpAddr>().unwrap());
    assert_eq!(result[1].address, "192.168.1.11".parse::<IpAddr>().unwrap());
    assert_eq!(result[2].adapter, "B");
}

#[test]
fn labels_use_friendly_name_and_fall_back_to_system_name() {
    let mut adapter = interface();
    adapter.friendly_name = Some("Wi-Fi".to_owned());
    assert_eq!(snapshot(&adapter)[0].adapter, "Wi-Fi");
    adapter.friendly_name = Some(" ".to_owned());
    assert_eq!(snapshot(&adapter)[0].adapter, "adapter");
}

#[test]
fn ipv6_flags_and_scopes_exclude_unusable_addresses() {
    let mut adapter = interface();
    adapter.ipv4.clear();
    adapter.ipv6 = (1..=6)
        .map(|n| format!("fd00::{n}/64").parse().unwrap())
        .collect();
    adapter.ipv6_scope_ids = vec![0, 0, 0, 0, 42];
    adapter.ipv6_addr_flags = vec![
        Ipv6AddrFlags {
            tentative: true,
            ..Default::default()
        },
        Ipv6AddrFlags {
            duplicated: true,
            ..Default::default()
        },
        Ipv6AddrFlags {
            deprecated: true,
            ..Default::default()
        },
        Ipv6AddrFlags {
            temporary: true,
            ..Default::default()
        },
        Ipv6AddrFlags::default(),
    ];
    let result = snapshot(&adapter);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].address, "fd00::4".parse::<IpAddr>().unwrap());
    assert_eq!(result[1].address, "fd00::6".parse::<IpAddr>().unwrap());
}

#[test]
fn only_private_ipv6_or_cgnat_still_has_a_recommendation() {
    for address in ["fd00::1", "100.64.0.1"] {
        let result = select(vec![candidate("VPN", address)]);
        assert_eq!(result.len(), lan().len());
        assert!(result[0].recommended);
        assert!(!result[1].recommended);
    }
}
