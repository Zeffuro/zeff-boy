use super::*;

pub(in super::super) fn parse_cue_bytes(cue_bytes: &[u8]) -> Result<CueSheet, PceCdLoadError> {
    if cue_bytes.len() > PCE_CD_CUE_BYTES_LIMIT {
        return Err(PceCdLoadError::CueTooLarge(cue_bytes.len() as u64));
    }
    let cue = std::str::from_utf8(cue_bytes).map_err(|_| PceCdLoadError::CueNotUtf8)?;
    parse_cue(cue)
}

pub(super) fn parse_cue(cue: &str) -> Result<CueSheet, PceCdLoadError> {
    let mut files: Vec<CueFile> = Vec::new();
    let mut tracks: Vec<CueTrack> = Vec::new();
    let mut current_file = None;
    for (line_index, source) in cue.lines().enumerate() {
        let line_number = line_index + 1;
        let line = source.trim();
        if line.is_empty() {
            continue;
        }
        let keyword = line.split_ascii_whitespace().next().unwrap();
        if keyword.eq_ignore_ascii_case("REM") {
            continue;
        }
        if keyword.eq_ignore_ascii_case("FILE") {
            if files.len() == PCE_CD_FILE_REFERENCE_LIMIT {
                return Err(PceCdLoadError::TooManyFileReferences);
            }
            let reference = parse_file_reference(line, line_number)?;
            if files
                .iter()
                .any(|file| file.reference.eq_ignore_ascii_case(reference.as_str()))
            {
                return Err(PceCdLoadError::DuplicateFile);
            }
            current_file = Some(files.len());
            files.push(CueFile {
                reference,
                track_indices: Vec::new(),
            });
            continue;
        }
        if keyword.eq_ignore_ascii_case("TRACK") {
            let file_index = current_file.ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let expected_number = match tracks.last() {
                None => 1,
                Some(track) => track
                    .number
                    .checked_add(1)
                    .ok_or(PceCdLoadError::InvalidTrackOrder)?,
            };
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let number: u8 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .filter(|number| (1..=99).contains(number))
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if number != expected_number {
                return Err(PceCdLoadError::InvalidTrackOrder);
            }
            if tracks.iter().any(|track| track.number == number) {
                return Err(PceCdLoadError::DuplicateTrack(number));
            }
            let mode = match fields.next().map(str::to_ascii_uppercase).as_deref() {
                Some("MODE1/2352") => CdTrackMode::Mode1_2352,
                Some("MODE1/2048") => CdTrackMode::Mode1_2048,
                Some("AUDIO") => CdTrackMode::Audio,
                Some(mode) => return Err(PceCdLoadError::UnsupportedTrackMode(mode.to_owned())),
                None => return Err(PceCdLoadError::MalformedLine(line_number)),
            };
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            files[file_index].track_indices.push(tracks.len());
            tracks.push(CueTrack {
                number,
                file_index,
                mode,
                index0: None,
                index1: None,
                pregap: None,
            });
            continue;
        }
        if keyword.eq_ignore_ascii_case("PREGAP") {
            let track = tracks
                .last_mut()
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if track.index0.is_some() || track.index1.is_some() {
                return Err(PceCdLoadError::InvalidIndexOrder(track.number));
            }
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let pregap = fields
                .next()
                .and_then(parse_msf)
                .filter(|pregap| *pregap != 0)
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            if track.pregap.replace(pregap).is_some() {
                return Err(PceCdLoadError::DuplicatePregap(track.number));
            }
            continue;
        }
        if keyword.eq_ignore_ascii_case("INDEX") {
            let track = tracks
                .last_mut()
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let index: u8 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .filter(|index| matches!(index, 0 | 1))
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let lba = fields
                .next()
                .and_then(parse_msf)
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            let destination = if index == 0 {
                if track.index1.is_some() || track.pregap.is_some() {
                    return Err(PceCdLoadError::InvalidIndexOrder(track.number));
                }
                &mut track.index0
            } else {
                &mut track.index1
            };
            if destination.replace(lba).is_some() {
                return Err(PceCdLoadError::DuplicateIndex {
                    track: track.number,
                    index,
                });
            }
            continue;
        }
        if !matches!(
            keyword.to_ascii_uppercase().as_str(),
            "CATALOG" | "TITLE" | "PERFORMER" | "SONGWRITER"
        ) {
            return Err(PceCdLoadError::MalformedLine(line_number));
        }
    }
    if files.is_empty() {
        return Err(PceCdLoadError::MissingFile);
    }
    if tracks.is_empty() || files.iter().any(|file| file.track_indices.is_empty()) {
        return Err(PceCdLoadError::InvalidTrackOrder);
    }
    Ok(CueSheet { files, tracks })
}

fn parse_file_reference(line: &str, line_number: usize) -> Result<String, PceCdLoadError> {
    let arguments = line
        .get(4..)
        .ok_or(PceCdLoadError::MalformedLine(line_number))?
        .trim_start();
    let remainder = arguments
        .strip_prefix('"')
        .ok_or(PceCdLoadError::MalformedLine(line_number))?;
    let end = remainder
        .find('"')
        .ok_or(PceCdLoadError::MalformedLine(line_number))?;
    if !remainder[end + 1..].trim().eq_ignore_ascii_case("BINARY") {
        return Err(PceCdLoadError::UnsupportedFileType(
            remainder[end + 1..].trim().to_owned(),
        ));
    }
    normalize_portable_path(&remainder[..end])
        .map_err(|_| PceCdLoadError::UnsafeFileReference(remainder[..end].to_owned()))
}

pub(in super::super) fn normalize_portable_path(value: &str) -> Result<String, ()> {
    if value.is_empty()
        || value.len() > PCE_CD_PATH_BYTES_LIMIT
        || value.contains('\0')
        || value.contains(':')
        || value.starts_with('/')
        || value.starts_with('\\')
    {
        return Err(());
    }
    let replaced = value.replace('\\', "/");
    if replaced.starts_with('/') || replaced.ends_with('/') {
        return Err(());
    }
    let components = replaced.split('/').collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > PCE_CD_PATH_DEPTH_LIMIT
        || components.iter().any(|component| {
            component.is_empty()
                || matches!(*component, "." | "..")
                || component.len() > PCE_CD_PATH_COMPONENT_BYTES_LIMIT
        })
    {
        return Err(());
    }
    Ok(components.join("/"))
}

pub(in super::super) fn cue_track_layout(
    sheet: &CueSheet,
    file_bytes: &[usize],
) -> Result<Vec<Vec<CueTrackLayout>>, PceCdLoadError> {
    if file_bytes.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }

    let mut cursor = 0_u32;
    let mut layout = Vec::with_capacity(sheet.files.len());
    for (file_index, (cue_file, &file_bytes)) in sheet.files.iter().zip(file_bytes).enumerate() {
        let file_tracks = cue_file
            .track_indices
            .iter()
            .map(|&index| &sheet.tracks[index])
            .collect::<Vec<_>>();
        let first_track = file_tracks[0];
        let sector_len = sector_bytes(first_track.mode);
        if file_tracks
            .iter()
            .any(|track| sector_bytes(track.mode) != sector_len)
        {
            return Err(PceCdLoadError::MixedSectorSizes);
        }
        if !file_bytes.is_multiple_of(sector_len) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: file_bytes,
                sector_bytes: sector_len,
            });
        }
        let total_sectors = u32::try_from(file_bytes / sector_len)
            .map_err(|_| PceCdLoadError::TrackOutsideBin(first_track.number))?;
        let anchor = if file_index == 0 {
            first_track
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(first_track.number))?
        } else {
            first_track
                .index0
                .or(first_track.index1)
                .ok_or(PceCdLoadError::MissingIndex1(first_track.number))?
        };
        if anchor >= total_sectors {
            return Err(PceCdLoadError::TrackOutsideBin(first_track.number));
        }

        let base = cursor;
        let mut virtual_offset = 0_u32;
        let mut file_layout = Vec::with_capacity(file_tracks.len());
        for (track_offset, &&track) in file_tracks.iter().enumerate() {
            debug_assert_eq!(track.file_index, file_index);
            let raw_index1 = track
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(track.number))?;
            if track.index0.is_some_and(|index0| index0 > raw_index1) {
                return Err(PceCdLoadError::InvalidIndexOrder(track.number));
            }
            let end = file_tracks
                .get(track_offset + 1)
                .map(|next| next.index0.unwrap_or(next.index1.unwrap_or(u32::MAX)))
                .unwrap_or(total_sectors);
            if raw_index1 >= end || end > total_sectors {
                return Err(PceCdLoadError::TrackOutsideBin(track.number));
            }
            let virtual_pregap = track.pregap.unwrap_or(0);
            let index1 = raw_index1
                .checked_sub(anchor)
                .and_then(|index| base.checked_add(index))
                .and_then(|index| index.checked_add(virtual_offset))
                .and_then(|index| index.checked_add(virtual_pregap))
                .ok_or(PceCdLoadError::InvalidTrackOrder)?;
            let index0 = if virtual_pregap != 0 {
                Some(
                    index1
                        .checked_sub(virtual_pregap)
                        .ok_or(PceCdLoadError::InvalidTrackOrder)?,
                )
            } else {
                track
                    .index0
                    .and_then(|index| index.checked_sub(anchor))
                    .map(|index| {
                        base.checked_add(index)
                            .and_then(|index| index.checked_add(virtual_offset))
                            .ok_or(PceCdLoadError::InvalidTrackOrder)
                    })
                    .transpose()?
            };
            let raw_stored_start = if virtual_pregap == 0 {
                index0.and(track.index0).unwrap_or(raw_index1)
            } else {
                raw_index1
            };
            let source_bytes = raw_stored_start as usize * sector_len..end as usize * sector_len;
            file_layout.push(CueTrackLayout {
                track,
                index0,
                index1,
                stored_start: if virtual_pregap != 0 {
                    index1
                } else {
                    index0.unwrap_or(index1)
                },
                source_bytes,
                virtual_pregap: virtual_pregap != 0,
            });
            virtual_offset = virtual_offset
                .checked_add(virtual_pregap)
                .ok_or(PceCdLoadError::InvalidTrackOrder)?;
        }
        cursor = cursor
            .checked_add(
                total_sectors
                    .checked_sub(anchor)
                    .ok_or(PceCdLoadError::InvalidTrackOrder)?,
            )
            .and_then(|cursor| cursor.checked_add(virtual_offset))
            .ok_or(PceCdLoadError::TrackOutsideBin(first_track.number))?;
        layout.push(file_layout);
    }
    Ok(layout)
}

pub(super) fn portable_path(reference: &str) -> PathBuf {
    reference.split('/').collect()
}

pub(super) fn sector_bytes(mode: CdTrackMode) -> usize {
    match mode {
        CdTrackMode::Mode1_2048 => 2_048,
        CdTrackMode::Mode1_2352 => 2_352,
        CdTrackMode::Audio => 2_352,
    }
}

fn parse_msf(value: &str) -> Option<u32> {
    let mut fields = value.split(':');
    let minutes: u32 = fields.next()?.parse().ok()?;
    let seconds: u32 = fields.next()?.parse().ok()?;
    let frames: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || seconds >= 60 || frames >= 75 {
        return None;
    }
    minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(75)?
        .checked_add(frames)
}
