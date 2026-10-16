use super::*;
use std::sync::OnceLock;
#[path = "../physical_status_register_first/check.rs"]
mod check;
#[path = "../physical_status_register_first/decode.rs"]
mod decode;
#[path = "../physical_status_register_first/default_bus.rs"]
mod default_bus;
#[path = "../physical_status_register_first/device.rs"]
mod device;
#[path = "../physical_status_register_first/goldens.rs"]
mod goldens;
#[path = "../physical_status_register_first/model.rs"]
mod model;
#[path = "../physical_status_register_first/value.rs"]
mod value;

pub(super) struct Seed {
    pub r: Registers,
    pub queue: [u16; 2],
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
}
pub(super) fn seeds() -> &'static [Seed] {
    static SEEDS: OnceLock<Vec<Seed>> = OnceLock::new();
    SEEDS.get_or_init(|| {
        let rows: Vec<_> = decode::cases()
            .into_iter()
            .filter(|c| c.label.starts_with("complete-c"))
            .map(|c| Seed {
                r: c.initial.registers,
                queue: c.initial.queue.unwrap(),
                clock: c.initial.clock,
                words: c.memory.into_iter().collect(),
            })
            .collect();
        assert_eq!(rows.len(), 27);
        rows
    })
}
pub(super) fn run() -> (u32, u32) {
    let (count, bad, unsupported) = goldens::run();
    (count, bad + u32::from(unsupported != 9))
}
