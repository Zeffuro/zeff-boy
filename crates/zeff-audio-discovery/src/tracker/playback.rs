const PERIODS: &[u16] = &[
    1712, 1616, 1524, 1440, 1356, 1280, 1208, 1140, 1076, 1016, 960, 906, 856, 808, 762, 720, 678,
    640, 604, 570, 538, 508, 480, 453, 428, 404, 381, 360, 339, 320, 302, 285, 269, 254, 240, 226,
    214, 202, 190, 180, 170, 160, 151, 143, 135, 127, 120, 113, 107, 101, 95, 90, 85, 80, 75, 71,
    67, 63, 60, 56,
];

pub(super) fn supported(data: &[u8], channels: u16) -> bool {
    if channels != 4 || !matches!(&data[1080..1084], b"M.K." | b"M!K!") {
        return false;
    }
    let patterns = usize::from(*data[952..1080].iter().max().unwrap()) + 1;
    let mut loop_start = None;
    let mut has_loop = false;
    let mut has_jump = false;
    for row in data[1084..1084 + patterns * 1024].as_chunks::<16>().0 {
        let mut row_loop = false;
        let mut jump = false;
        let mut pattern_break = false;
        for (channel, cell) in row.as_chunks::<4>().0.iter().enumerate() {
            let period = u16::from(cell[0] & 15) * 256 + u16::from(cell[1]);
            if period != 0 && !PERIODS.contains(&period) {
                return false;
            }
            let effect = cell[2] & 15;
            let value = cell[3];
            match effect {
                0xb => jump = true,
                0xd => {
                    if value >> 4 > 6 || value & 15 > 9 || (value >> 4) * 10 + (value & 15) > 63 {
                        return false;
                    }
                    pattern_break = true;
                }
                0xe => match value >> 4 {
                    0 if value != 1 => return false,
                    3 | 5 | 8 | 14 | 15 => return false,
                    4 | 7 if value & 15 > 7 || value & 3 == 3 => return false,
                    6 => {
                        if row_loop || patterns != 1 || data[950] != 1 {
                            return false;
                        }
                        row_loop = true;
                        has_loop = true;
                        if value & 15 == 0 {
                            if loop_start.replace(channel).is_some() {
                                return false;
                            }
                        } else if loop_start.take() != Some(channel) {
                            return false;
                        }
                    }
                    _ => {}
                },
                0xf if value == 0 => return false,
                _ => {}
            }
        }
        // The loader drops Dxx's row when Bxx shares its row.
        if jump && pattern_break {
            return false;
        }
        has_jump |= jump || pattern_break;
    }
    // Disjoint E6 blocks bound expansion to 64 * 16 rows; other modules to 128 * 64.
    loop_start.is_none() && !(has_loop && has_jump)
}
