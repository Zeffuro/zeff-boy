use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use netdev::Interface;
use netdev::interface::state::OperState;
use zeff_netplay::endpoint::ConnectionScope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostAddress {
    pub adapter: String,
    pub address: IpAddr,
    pub recommended: bool,
}

pub(crate) fn discover() -> Vec<HostAddress> {
    from_interfaces(netdev::get_interfaces())
}

pub(crate) fn run_if_requested() -> anyhow::Result<bool> {
    if !std::env::args().any(|arg| arg == "--netplay-addresses") {
        return Ok(false);
    }
    let rows: Vec<_> = discover()
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "adapter": row.adapter,
                "address": row.address.to_string(),
                "recommended": row.recommended,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(true)
}

struct Candidate {
    adapter: String,
    address: IpAddr,
    up: bool,
    active: bool,
    physical: bool,
    default: bool,
}

fn from_interfaces(interfaces: Vec<Interface>) -> Vec<HostAddress> {
    let mut labels = BTreeMap::<String, BTreeSet<(u32, String)>>::new();
    for interface in &interfaces {
        labels
            .entry(adapter_name(interface))
            .or_default()
            .insert((interface.index, interface.name.clone()));
    }
    let mut candidates = Vec::new();
    for interface in interfaces {
        let mut adapter = adapter_name(&interface);
        if labels[&adapter].len() > 1 {
            adapter = format!("{adapter} ({} #{})", interface.name, interface.index);
        }
        let up =
            interface.is_up() && matches!(interface.oper_state, OperState::Up | OperState::Unknown);
        if !up || interface.is_loopback() {
            continue;
        }
        append_candidates(
            &interface,
            adapter,
            interface.is_physical(),
            &mut candidates,
        );
    }
    select(candidates)
}

fn append_candidates(
    interface: &Interface,
    adapter: String,
    physical: bool,
    candidates: &mut Vec<Candidate>,
) {
    let up =
        interface.is_up() && matches!(interface.oper_state, OperState::Up | OperState::Unknown);
    if !up || interface.is_loopback() {
        return;
    }
    let active = interface.is_oper_up();
    let addresses = interface
        .ipv4
        .iter()
        .map(|net| IpAddr::V4(net.addr()))
        .chain(
            interface
                .ipv6
                .iter()
                .enumerate()
                .filter_map(|(index, net)| {
                    let flags = interface
                        .ipv6_addr_flags
                        .get(index)
                        .copied()
                        .unwrap_or_default();
                    let scoped = interface.ipv6_scope_ids.get(index).copied().unwrap_or(0) != 0;
                    (!scoped && !flags.tentative && !flags.duplicated && !flags.deprecated)
                        .then_some(IpAddr::V6(net.addr()))
                }),
        );
    for address in addresses {
        candidates.push(Candidate {
            adapter: adapter.clone(),
            address,
            up,
            active,
            physical,
            default: interface.default,
        });
    }
}

fn adapter_name(interface: &Interface) -> String {
    interface
        .friendly_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&interface.name)
        .to_owned()
}

fn select(mut candidates: Vec<Candidate>) -> Vec<HostAddress> {
    candidates.retain(|candidate| {
        candidate.up
            && !candidate.address.is_loopback()
            && ConnectionScope::TrustedPrivate
                .validate_bind(SocketAddr::new(candidate.address, 8766))
                .is_ok()
    });
    candidates.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.adapter.cmp(&b.adapter))
            .then_with(|| a.address.cmp(&b.address))
    });
    let mut seen = HashSet::new();
    let mut addresses = Vec::new();
    for candidate in candidates {
        if seen.insert((candidate.adapter.clone(), candidate.address)) {
            addresses.push(HostAddress {
                adapter: candidate.adapter,
                address: candidate.address,
                recommended: addresses.is_empty(),
            });
        }
    }
    addresses.push(HostAddress {
        adapter: "Loopback".to_owned(),
        address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        recommended: false,
    });
    addresses
}

fn rank(candidate: &Candidate) -> (u8, bool, bool) {
    let rfc1918 = matches!(candidate.address, IpAddr::V4(ip) if ip.is_private());
    let class = if rfc1918 && candidate.physical && candidate.active {
        0
    } else if rfc1918 && candidate.physical {
        1
    } else if rfc1918 {
        2
    } else if candidate.address.is_ipv4() {
        3
    } else {
        4
    };
    (class, !candidate.default, !candidate.active)
}

#[cfg(test)]
mod tests;
