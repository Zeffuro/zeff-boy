use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{Region, Vdp, VdpScheduleError};

const REGION: Region = Region::NtscUs;
const LINE_TICKS: u64 = 3420;
const H32_EDGE: u64 = 2660;
const H40_EDGE: u64 = 2640;
const HINT_MASK: u8 = 0x10;
const RELOAD_REGISTER: u8 = 10;
const SCROLL_REGISTER: u8 = 13;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}
fn write(device: &mut Vdp, index: u8, value: u8) {
    device
        .write_control(0x8000 | u16::from(index) << 8 | u16::from(value), REGION)
        .unwrap();
}
fn device(h40: bool) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    if h40 {
        write(&mut device, 12, 0x81);
        device.advance_to(tick(20), REGION).unwrap();
    }
    device
}

pub fn disjoint_table_and_counter_publications() {
    for (h40, edge) in [(false, H32_EDGE), (true, H40_EDGE)] {
        let mut device = device(h40);
        device.advance_to(tick(edge - 1), REGION).unwrap();
        let before = device.clone();
        assert_eq!(
            device.queue_register(SCROLL_REGISTER, 0x2e, REGION),
            Err(VdpScheduleError::CoincidentPublication(tick(edge)))
        );
        assert_eq!(device, before);
        write(&mut device, SCROLL_REGISTER, 0x2e);
        assert_eq!(device.registers().pending().unwrap().publish_at, tick(edge));
        let mut partitioned = device.clone();
        let mut counter = before;
        device.advance_to(tick(edge + 10), REGION).unwrap();
        partitioned.advance_to(tick(edge), REGION).unwrap();
        partitioned.advance_to(tick(edge + 10), REGION).unwrap();
        counter.advance_to(tick(edge + 10), REGION).unwrap();
        assert_eq!(device, partitioned);
        assert_eq!(device.interrupts(), counter.interrupts());
        assert_eq!(
            device.registers().registers()[usize::from(SCROLL_REGISTER)],
            Some(0x2e)
        );
        assert!(device.interrupts().horizontal_pending());
        assert!(device.registers().pending().is_none());
    }
}

pub fn old_reload_precedes_new_register() {
    for (h40, edge) in [(false, H32_EDGE), (true, H40_EDGE)] {
        let mut device = device(h40);
        device.advance_to(tick(21), REGION).unwrap();
        write(&mut device, RELOAD_REGISTER, 3);
        device.advance_to(tick(edge - 1), REGION).unwrap();
        write(&mut device, RELOAD_REGISTER, 7);
        device.advance_to(tick(edge), REGION).unwrap();
        assert_eq!(device.interrupts().line_counter(), 3);
        assert_eq!(
            device.registers().registers()[usize::from(RELOAD_REGISTER)],
            Some(7)
        );
        assert!(device.interrupts().horizontal_pending());
        for (line, expected) in [(1, 2), (2, 1), (3, 0), (4, 7)] {
            device
                .advance_to(tick(edge + line * LINE_TICKS), REGION)
                .unwrap();
            assert_eq!(device.interrupts().line_counter(), expected);
        }
    }
}

pub fn mask_publication_routes_existing_latch() {
    for (h40, edge) in [(false, H32_EDGE), (true, H40_EDGE)] {
        let mut device = device(h40);
        device.advance_to(tick(edge - 1), REGION).unwrap();
        assert_eq!(device.interrupt_level(), 0);
        write(&mut device, 0, HINT_MASK);
        assert_eq!(device.interrupt_level(), 0);
        device.advance_to(tick(edge), REGION).unwrap();
        assert!(device.interrupts().horizontal_pending());
        assert_eq!(device.interrupt_level(), 4);
    }
}

pub fn hv_latch_at_tied_publication() {
    for (h40, edge) in [(false, H32_EDGE), (true, H40_EDGE)] {
        let mut device = device(h40);
        let mut live = device.clone();
        device.advance_to(tick(edge - 1), REGION).unwrap();
        write(&mut device, 0, 2);
        device.advance_to(tick(edge), REGION).unwrap();
        live.advance_to(tick(edge), REGION).unwrap();
        let latched = live.read_hv(REGION).unwrap();
        assert_eq!(device.read_hv(REGION), Ok(latched));
        device.advance_to(tick(edge + 100), REGION).unwrap();
        live.advance_to(tick(edge + 100), REGION).unwrap();
        assert_eq!(device.read_hv(REGION), Ok(latched));
        assert_ne!(live.read_hv(REGION), Ok(latched));
        device
            .advance_to(tick(edge + LINE_TICKS - 1), REGION)
            .unwrap();
        write(&mut device, 0, 0);
        device.advance_to(tick(edge + LINE_TICKS), REGION).unwrap();
        live.advance_to(tick(edge + LINE_TICKS), REGION).unwrap();
        assert_eq!(device.read_hv(REGION), live.read_hv(REGION));
    }
}
