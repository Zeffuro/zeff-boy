use super::{Budget, GbMusyxSong, ReadError, profiles::Driver, require, span, spans};

pub(super) struct Project<'a> {
    pub bytes: &'a [u8],
    pub driver: Driver,
    pub macro_table: usize,
    pub macros: Vec<usize>,
    pub adsr_table: usize,
    pub adsrs: Vec<usize>,
    pub samples: Vec<(usize, usize)>,
    pub sample_map: Vec<u8>,
    pub song_table: usize,
    pub song_count: usize,
    pub ranges: Vec<(usize, usize)>,
}

pub(super) fn byte(bytes: &[u8], at: usize) -> Result<u8, ReadError> {
    bytes
        .get(at)
        .copied()
        .ok_or(ReadError::Invalid("data leaves the source"))
}

pub(super) fn le(bytes: &[u8], at: usize) -> Result<usize, ReadError> {
    Ok(usize::from(byte(bytes, at)?) | usize::from(byte(bytes, at + 1)?) << 8)
}

pub(super) fn be(bytes: &[u8], at: usize) -> Result<usize, ReadError> {
    Ok(usize::from(byte(bytes, at)?) << 8 | usize::from(byte(bytes, at + 1)?))
}

impl<'a> Project<'a> {
    pub fn new(
        bytes: &'a [u8],
        driver: Driver,
        budget: &mut Budget<'_>,
    ) -> Result<Self, ReadError> {
        budget.charge()?;
        let p = driver.project();
        let limit = (p / 0x4000 + 1) * 0x4000;
        require(p + 17 <= limit, "truncated project header")?;
        let macro_table = p + 15;
        let adsr_table = p + le(bytes, p)?;
        let sfx = p + le(bytes, p + 2)?;
        let sample_table = p + le(bytes, p + 5)?;
        let map = p + le(bytes, p + 9)?;
        let song_table = p + le(bytes, p + 12)?;
        let song_count = usize::from(byte(bytes, p + 14)?);
        let macro_count = le(bytes, macro_table)? / 2;
        require(
            (1..=256).contains(&macro_count)
                && le(bytes, macro_table)? == macro_count * 2
                && macro_table + macro_count * 2 <= adsr_table
                && adsr_table <= sfx
                && sfx <= sample_table
                && sample_table <= limit,
            "invalid project table order",
        )?;
        let mut ranges = vec![
            (driver.offset, p + 15),
            (
                driver.profile.bank0,
                driver.profile.bank0 + driver.profile.bank0_len,
            ),
            (macro_table, macro_table + macro_count * 2),
        ];
        let mut macros = Vec::new();
        for index in 0..macro_count {
            budget.charge()?;
            let at = macro_table + le(bytes, macro_table + index * 2)?;
            require(
                at >= macro_table + macro_count * 2 && at < adsr_table,
                "invalid macro entry",
            )?;
            macros.push(at);
        }
        let mut adsrs = Vec::new();
        if adsr_table != sfx {
            let count = le(bytes, adsr_table)? / 2;
            require(
                (1..=256).contains(&count) && le(bytes, adsr_table)? == count * 2,
                "invalid ADSR table",
            )?;
            ranges.push((adsr_table, adsr_table + count * 2));
            for index in 0..count {
                budget.charge()?;
                let at = adsr_table + le(bytes, adsr_table + index * 2)?;
                require(
                    at >= adsr_table + count * 2 && at + 7 <= sfx,
                    "invalid ADSR entry",
                )?;
                adsrs.push(at);
            }
        }
        require(
            sfx + usize::from(byte(bytes, p + 4)?) * 4 <= sample_table,
            "invalid effect table",
        )?;
        let sample_count = usize::from(byte(bytes, p + 7)?);
        require(
            sample_table + sample_count * 6 <= limit,
            "invalid sample table",
        )?;
        ranges.push((sample_table, sample_table + sample_count * 6));
        let mut samples = Vec::new();
        for index in 0..sample_count {
            budget.charge()?;
            let row = sample_table + index * 6;
            let cpu = le(bytes, row)?;
            let len = le(bytes, row + 2)? * 16;
            let bank = usize::from(driver.bank) + usize::from(byte(bytes, row + 5)?);
            require(
                (0x4000..0x8000).contains(&cpu)
                    && cpu.is_multiple_of(16)
                    && len > 0
                    && bank < 256
                    && byte(bytes, row + 4)? <= 1,
                "invalid sample mapping",
            )?;
            let at = bank * 0x4000 + cpu - 0x4000;
            require(
                at + len <= bytes.len().min(256 * 0x4000),
                "sample crosses driver bank range",
            )?;
            samples.push((at, at + len));
        }
        let map_count = usize::from(byte(bytes, p + 11)?);
        let mut sample_map = Vec::new();
        if map_count != 0 {
            require(
                map >= macro_table + macro_count * 2 && map + map_count <= adsr_table,
                "invalid sample map",
            )?;
            for index in 0..map_count {
                budget.charge()?;
                let sample = byte(bytes, map + index)?;
                require(usize::from(sample) < sample_count, "invalid mapped sample")?;
                sample_map.push(sample);
            }
            ranges.push((map, map + map_count));
        }
        require(
            song_table >= sample_table + sample_count * 6 && song_table + song_count * 3 <= limit,
            "invalid song table",
        )?;
        Ok(Self {
            bytes,
            driver,
            macro_table,
            macros,
            adsr_table,
            adsrs,
            samples,
            sample_map,
            song_table,
            song_count,
            ranges,
        })
    }

    pub fn song(&self, index: usize, budget: &mut Budget<'_>) -> Result<GbMusyxSong, ReadError> {
        require(
            index < self.song_count,
            "song selector is outside its table",
        )?;
        budget.charge()?;
        let table = self.song_table + index * 3;
        let bank = usize::from(self.driver.bank) + usize::from(byte(self.bytes, table)?);
        let cpu = le(self.bytes, table + 1)?;
        require(
            bank < 256 && (0x4000..=0x7f6e).contains(&cpu),
            "invalid song mapping",
        )?;
        let offset = bank * 0x4000 + cpu - 0x4000;
        let end = (bank + 1) * 0x4000;
        require(end <= self.bytes.len(), "song leaves the source")?;
        let mut ranges = self.ranges.clone();
        ranges.extend([(table, table + 3), (offset, offset + 146)]);
        let (tracks, roots) = super::sequence::inspect(self, offset, end, &mut ranges, budget)?;
        require(
            tracks.iter().any(|track| track.note_count != 0),
            "selector has no notes",
        )?;
        super::macros::inspect(self, roots, &mut ranges, budget)?;
        Ok(GbMusyxSong {
            profile: self.driver.profile.name,
            index: index as u16,
            title: format!("GB MusyX song {index}"),
            bank: bank as u8,
            table_entry: span(table, 3),
            header: span(offset, 146),
            tracks,
            mapped_spans: spans(ranges),
            warnings: vec!["Native CGB playback uses the embedded selector for the requested duration; song end, loop length and soundtrack completeness are not qualified.".into()],
        })
    }

    pub fn roots(&self, index: u8, voice: u8) -> Result<Vec<(usize, u8)>, ReadError> {
        let at = *self
            .macros
            .get(usize::from(index))
            .ok_or(ReadError::Invalid("invalid macro selector"))?;
        let voice = if byte(self.bytes, at)? == 0x0e {
            let parameter = byte(self.bytes, at + 1)?;
            if parameter & 0x10 != 0 {
                return Ok(vec![(at, 0), (at, 1)]);
            }
            parameter & 0x7f
        } else {
            voice
        };
        require(voice < 4, "invalid macro voice")?;
        Ok(vec![(at, voice)])
    }
}
