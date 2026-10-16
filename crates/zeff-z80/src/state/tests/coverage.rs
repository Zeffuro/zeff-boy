use super::*;

pub(super) fn record(bus: &BusState, visited: &mut BTreeSet<String>) {
    let mut w = Writer::new();
    bus.write_continuation(&mut w);
    let bytes = w.into_bytes();
    let mut r = Reader::new(&bytes);
    if r.read_bool().unwrap() {
        visited.insert("prefix".into());
        r.read_u16().unwrap();
        r.read_u8().unwrap();
    }
    if r.read_bool().unwrap() {
        r.read_u16().unwrap();
        let operation = r.read_u8().unwrap();
        operation_phase(operation, &mut r, visited);
    }
}

fn phase(family: &str, r: &mut Reader<'_>, visited: &mut BTreeSet<String>) -> u8 {
    let phase = r.read_u8().unwrap();
    visited.insert(format!("{family}.{phase}"));
    phase
}

fn operation_phase(operation: u8, r: &mut Reader<'_>, visited: &mut BTreeSet<String>) {
    match operation {
        0 => {
            visited.insert("single".into());
        }
        1 => {
            phase("fetch", r, visited);
            phase("load", r, visited);
        }
        2 => {
            let pair = r.read_u8().unwrap();
            if pair < 2 {
                r.read_u8().unwrap();
            }
            r.read_u16().unwrap();
            read_option_u16(r).unwrap();
            phase("fetch", r, visited);
            phase("stack", r, visited);
        }
        3 => {
            r.read_u16().unwrap();
            read_option_u8(r).unwrap();
            phase("rmw", r, visited);
        }
        4 => {
            if phase("indexed", r, visited) == 4 {
                operation_phase(3, r, visited);
            }
        }
        5 => {
            if phase("indexed_cb", r, visited) == 2 {
                operation_phase(3, r, visited);
            }
        }
        6 => {
            visited.insert("cb_fetch".into());
        }
        7 => {
            visited.insert("ed_fetch".into());
        }
        8 => {
            r.read_u8().unwrap();
            phase("internal", r, visited);
        }
        9 => {
            visited.insert("io".into());
        }
        10 => {
            let mut fields = [0; 8];
            r.read_exact(&mut fields).unwrap();
            phase("block", r, visited);
        }
        11 => {
            r.read_u16().unwrap();
            if r.read_u8().unwrap() == 1 {
                r.read_u8().unwrap();
                r.read_u8().unwrap();
            }
            match phase("interrupt", r, visited) {
                1 => operation_phase(1, r, visited),
                2 => operation_phase(2, r, visited),
                _ => {}
            }
        }
        _ => panic!("unknown operation {operation}"),
    }
}

pub(super) fn require_phases(
    visited: &BTreeSet<String>,
    family: &str,
    phases: std::ops::Range<u8>,
) {
    for phase in phases {
        assert!(
            visited.contains(&format!("{family}.{phase}")),
            "missing {family}.{phase}: {visited:?}"
        );
    }
}
