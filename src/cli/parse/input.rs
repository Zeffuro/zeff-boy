use super::super::types::{HeadlessInputEvent, HeadlessZapperEvent};
use super::numbers::{parse_frame_range_arg, parse_u64_arg};

fn parse_input_keys(value: &str, flag: &str) -> anyhow::Result<(u8, u8, bool)> {
    let mut buttons = 0u8;
    let mut dpad = 0u8;
    let mut reset = false;
    let mut parsed_any = false;

    for raw_key in value.split(|c: char| c == '+' || c == '|' || c.is_whitespace()) {
        let key = raw_key.trim().to_ascii_lowercase().replace(['_', '-'], "");
        if key.is_empty() {
            continue;
        }
        parsed_any = true;
        match key.as_str() {
            "a" | "i" => buttons |= 0x01,
            "b" | "ii" => buttons |= 0x02,
            "select" | "sel" => buttons |= 0x04,
            "start" | "run" => buttons |= 0x08,
            "l" | "leftshoulder" | "shoulderl" | "iii" => buttons |= 0x10,
            "r" | "rightshoulder" | "shoulderr" | "iv" => buttons |= 0x20,
            "x" | "v" => buttons |= 0x40,
            "y" | "vi" => buttons |= 0x80,
            "right" => dpad |= 0x01,
            "left" => dpad |= 0x02,
            "up" => dpad |= 0x04,
            "down" => dpad |= 0x08,
            "reset" | "softreset" | "softresetbutton" => reset = true,
            _ => anyhow::bail!(
                "{flag} has unknown key {:?}; expected a/i,b/ii,select,start/run,l/iii,r/iv,x/v,y/vi,up,down,left,right,reset",
                raw_key
            ),
        }
    }

    if !parsed_any {
        anyhow::bail!("{flag} requires at least one key");
    }

    Ok((buttons, dpad, reset))
}

pub(super) fn parse_input_event_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<Vec<HeadlessInputEvent>> {
    let mut events = Vec::new();
    for raw_event in value.split(',') {
        let event = raw_event.trim();
        if event.is_empty() {
            continue;
        }

        let (keys_raw, range_raw) = if let Some((keys, range)) = event.split_once('@') {
            (keys.trim(), range.trim())
        } else if let Some((left, right)) = event.split_once(':') {
            if left
                .trim()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
            {
                (right.trim(), left.trim())
            } else {
                (left.trim(), right.trim())
            }
        } else {
            anyhow::bail!("{flag} event must be key@start-end, key:start-end, or start-end:key");
        };

        let (buttons, dpad, reset) = parse_input_keys(keys_raw, flag)?;
        let (start_frame, end_frame) = parse_frame_range_arg(range_raw, flag)?;
        events.push(HeadlessInputEvent {
            start_frame,
            end_frame,
            buttons,
            dpad,
            reset,
        });
    }

    if events.is_empty() {
        anyhow::bail!("{flag} did not contain any input events");
    }

    Ok(events)
}

pub(super) fn parse_zapper_event_arg(
    value: &str,
    flag: &str,
) -> anyhow::Result<Vec<HeadlessZapperEvent>> {
    let mut events = Vec::new();
    for raw_event in value.split(';') {
        let event = raw_event.trim();
        if event.is_empty() {
            continue;
        }

        let (range_raw, pos_raw, mode_raw) = if let Some((left, pos)) = event.split_once(':') {
            if let Some((mode, range)) = left.split_once('@') {
                (range.trim(), pos.trim(), mode.trim())
            } else {
                (left.trim(), pos.trim(), "hit")
            }
        } else {
            anyhow::bail!(
                "{flag} event must be start-end:x,y or mode@start-end:x,y; separate multiple events with semicolons; modes: hit, miss, trigger"
            );
        };

        let separator = pos_raw
            .find([',', 'x', 'X', ';', '/'])
            .ok_or_else(|| anyhow::anyhow!("{flag} zapper position must be x,y"))?;
        let (x_raw, y_with_sep) = pos_raw.split_at(separator);
        let y_raw = &y_with_sep[1..];
        let x = parse_u64_arg(x_raw.trim(), flag)?;
        let y = parse_u64_arg(y_raw.trim(), flag)?;
        if x > u16::MAX as u64 || y > u16::MAX as u64 {
            anyhow::bail!("{flag} zapper coordinates must fit in u16");
        }

        let mode = mode_raw.to_ascii_lowercase().replace(['_', '-'], "");
        let (trigger, hit) = match mode.as_str() {
            "hit" | "fire" | "triggerhit" => (true, true),
            "miss" | "triggermiss" => (true, false),
            "trigger" | "light" | "sense" => (true, true),
            "aim" | "idle" => (false, false),
            _ => anyhow::bail!(
                "{flag} unknown zapper mode {mode_raw:?}; expected hit, miss, trigger, or aim"
            ),
        };

        let (start_frame, end_frame) = parse_frame_range_arg(range_raw, flag)?;
        events.push(HeadlessZapperEvent {
            start_frame,
            end_frame,
            x: x as u16,
            y: y as u16,
            trigger,
            hit,
        });
    }

    if events.is_empty() {
        anyhow::bail!("{flag} did not contain any zapper events");
    }

    Ok(events)
}

pub(super) fn parse_input_script(path: &str) -> anyhow::Result<Vec<HeadlessInputEvent>> {
    let contents = std::fs::read_to_string(path)
        .map_err(|err| anyhow::anyhow!("failed to read --input-script '{}': {}", path, err))?;
    let mut events = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        events.extend(
            parse_input_event_arg(line, "--input-script")
                .map_err(|err| anyhow::anyhow!("{} at {}:{}", err, path, index + 1))?,
        );
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pce_button_names_alias_the_generic_host_buttons() {
        let events = parse_input_event_arg("i+ii+iii+iv+v+vi+run@10-11", "--press").unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].buttons, 0xFB);
        assert_eq!((events[0].start_frame, events[0].end_frame), (10, 11));
    }
}
