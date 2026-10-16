use super::*;

pub(super) fn section(section: u32) -> (u32, u32) {
    let sizes: &[u16] = match section {
        1 => &[0, 0x40, 0x80],
        2 => &[0xc0, 0x1c0],
        _ => panic!("classification section"),
    };
    let mut rows = decode::rows();
    let row = &mut rows[1];
    assert_eq!(
        (row.opcode, row.final_fetch, row.outcome),
        (0xb050, true, 0)
    );
    let mut count = 0;
    let mut bad = 0;
    for &size in sizes {
        for destination in 0..8 {
            for mode in 0..8 {
                if size == 0 && mode == 1 {
                    continue;
                }
                for register in 0..if mode == 7 { 5 } else { 8 } {
                    let next = 0xb000 | destination << 9 | size | mode << 3 | register;
                    row.id = 64 + u64::from(count);
                    row.initial_queue[1] = next;
                    row.initial_memory.insert(0x452002, next);
                    row.memory.insert(0x452002, next);
                    row.capture.ird = next;
                    row.ssw = next & 0xffe0 | 18;
                    for (delta, value) in [(8, next), (14, row.ssw)] {
                        let address = row.initial.ssp.wrapping_sub(delta);
                        row.memory.insert(address & 0xfffffe, value);
                        let event = row
                            .events
                            .iter_mut()
                            .find(|e| e.tag == 2 && e.access.logical_address == address)
                            .unwrap();
                        event.value = value;
                    }
                    count += 1;
                    bad += u32::from(!check::run(row));
                }
            }
        }
    }
    assert_eq!(count, if section == 1 { 1400 } else { 976 });
    (count, bad)
}
