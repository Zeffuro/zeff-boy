use super::{EngineSoftwareSong, ReadError, ReadResult, RomSpan, ScanStop};

pub(super) struct GraphBudget {
    remaining: usize,
}

pub(super) fn push_song(
    songs: &mut Vec<EngineSoftwareSong>,
    song: EngineSoftwareSong,
    retained: &mut usize,
    limit: usize,
) -> Result<(), ScanStop> {
    let owned = song_owned_bytes(&song);
    let available = limit
        .checked_sub(*retained)
        .and_then(|bytes| bytes.checked_sub(owned))
        .ok_or(ScanStop::InventoryLimit)?;
    let mut added_capacity = 0;
    if songs.len() == songs.capacity() {
        if available < size_of::<EngineSoftwareSong>() {
            return Err(ScanStop::InventoryLimit);
        }
        let previous = songs.capacity();
        songs
            .try_reserve_exact(1)
            .map_err(|_| ScanStop::InventoryLimit)?;
        added_capacity = (songs.capacity() - previous) * size_of::<EngineSoftwareSong>();
        if added_capacity > available {
            return Err(ScanStop::InventoryLimit);
        }
    }
    *retained += owned + added_capacity;
    songs.push(song);
    Ok(())
}

pub(super) fn retained_owned_bytes(songs: &Vec<EngineSoftwareSong>) -> usize {
    songs.iter().map(song_owned_bytes).fold(
        songs
            .capacity()
            .saturating_mul(size_of::<EngineSoftwareSong>()),
        usize::saturating_add,
    )
}

fn song_owned_bytes(song: &EngineSoftwareSong) -> usize {
    song.title
        .capacity()
        .saturating_add(
            song.mapped_spans
                .capacity()
                .saturating_mul(size_of::<RomSpan>()),
        )
        .saturating_add(song.warnings.iter().map(|warning| warning.capacity()).fold(
            song.warnings.capacity().saturating_mul(size_of::<String>()),
            usize::saturating_add,
        ))
}

impl GraphBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    pub(super) fn reserve<T>(&mut self, values: &mut Vec<T>, additional: usize) -> ReadResult<()> {
        let required = values
            .len()
            .checked_add(additional)
            .ok_or(ReadError::Stop(ScanStop::InventoryLimit))?;
        let previous = values.capacity();
        if required <= previous {
            return Ok(());
        }
        self.charge::<T>(required - previous)?;
        values
            .try_reserve_exact(additional)
            .map_err(|_| ReadError::Stop(ScanStop::InventoryLimit))?;
        self.charge::<T>(values.capacity() - required)
    }

    fn charge<T>(&mut self, count: usize) -> ReadResult<()> {
        self.remaining = count
            .checked_mul(size_of::<T>())
            .and_then(|bytes| self.remaining.checked_sub(bytes))
            .ok_or(ReadError::Stop(ScanStop::InventoryLimit))?;
        Ok(())
    }
}
