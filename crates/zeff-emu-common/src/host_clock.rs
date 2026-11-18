use std::time::SystemTime;

use jiff::{Timestamp, Zoned};

pub fn now() -> Zoned {
    local_time(Timestamp::now())
}

pub fn at(time: SystemTime) -> Option<Zoned> {
    Timestamp::try_from(time).ok().map(local_time)
}

#[cfg(windows)]
fn local_time(timestamp: Timestamp) -> Zoned {
    use jiff::tz::Offset;
    use time::{OffsetDateTime, UtcOffset};

    // Use Windows' effective offset, including local daylight-saving settings.
    let offset = OffsetDateTime::from_unix_timestamp_nanos(timestamp.as_nanosecond())
        .ok()
        .and_then(|utc| UtcOffset::local_offset_at(utc).ok())
        .and_then(|offset| Offset::from_seconds(offset.whole_seconds()).ok())
        .unwrap_or(Offset::UTC);
    timestamp.to_zoned(offset.to_time_zone())
}

#[cfg(not(windows))]
fn local_time(timestamp: Timestamp) -> Zoned {
    timestamp.to_zoned(jiff::tz::TimeZone::system())
}
