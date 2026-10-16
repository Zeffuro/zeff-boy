use super::*;

pub(super) fn witnesses(count: &mut u32, bad: &mut u32) {
    for rooms in [[0u64, 1], [2, 3]] {
        for context in 0..3 {
            for room in rooms {
                let sr = [0x0710, 0x2710, 0x0010][context];
                let mut c = goldens::post(context, 0x5810, sr);
                let start = u64::MAX - u64::from(u32::MAX) - room;
                c.bus.clock = start;
                c.bus.elapsed[0] = u32::MAX;
                let t = c.transfers[0];
                let mut events = vec![Event::Read(t.access, start, 0x5810)];
                let mut r = t.r;
                let end = if room < 2 {
                    u64::MAX - room
                } else {
                    events.push(Event::Idle(u64::MAX - room, 2));
                    r = status(r, sr);
                    u64::MAX - room + 2
                };
                let good = check::capacity(c, r, end, events, 1);
                goldens::record(count, bad, good, if room < 2 { "L09" } else { "L10" });
            }
        }
    }
    for context in 0..3 {
        let sr = [0x0710, 0x2710, 0x0010][context];
        let mut c = goldens::post(context, 0x5810, sr);
        let start = u64::MAX - u64::from(u32::MAX) - 4;
        c.bus.clock = start;
        c.bus.elapsed[0] = u32::MAX;
        let t = c.transfers[0];
        let r = status(t.r, sr);
        let events = vec![
            Event::Read(t.access, start, 0x5810),
            Event::Idle(u64::MAX - 4, 2),
            Event::Idle(u64::MAX - 2, 2),
        ];
        goldens::record(
            count,
            bad,
            check::capacity(c, r, u64::MAX, events, 1),
            "L11",
        );
    }
    for context in 0..3 {
        let sr = [0x0710, 0x2710, 0x0010][context];
        let mut c = goldens::post(context, 0x5810, sr);
        let start = u64::MAX - u64::from(u32::MAX) - 8;
        c.bus.clock = start;
        c.bus.elapsed[1] = u32::MAX;
        let source = c.transfers[0];
        let first = c.transfers[1];
        let events = vec![
            Event::Read(source.access, start, 0x5810),
            Event::Idle(start + 4, 2),
            Event::Idle(start + 6, 2),
            Event::Read(first.access, start + 8, 0x4e71),
        ];
        goldens::record(
            count,
            bad,
            check::capacity(c, first.r, u64::MAX, events, 2),
            "L12",
        );
    }
    for context in 0..3 {
        for room in [1u64, 2] {
            let mut r = registers(if context == 0 { 0x071b } else { 0x271b });
            if context == 0 {
                r.usp = 0x3002;
                r.ssp = 0x5000;
            } else {
                r.ssp = 0x3002;
            }
            let opcode = if context == 2 { 0x46e7 } else { 0x44e7 };
            let mut c = Case::new(opcode, 0x4afc, r);
            let start = u64::MAX - room;
            c.bus.clock = start;
            let events = if room == 1 {
                vec![]
            } else {
                vec![Event::Idle(start, 2)]
            };
            let end = if room == 1 { u64::MAX - 1 } else { u64::MAX };
            goldens::record(count, bad, check::capacity(c, r, end, events, 0), "L13");
        }
    }
}
