use super::super::types::{HeadlessBusTraceAccess, HeadlessBusTraceFilter};
use super::numbers::parse_addr_arg;

pub(super) fn parse_addr_range_list_arg(
    value: &str,
    flag: &str,
    access: HeadlessBusTraceAccess,
) -> anyhow::Result<Vec<HeadlessBusTraceFilter>> {
    let mut filters = Vec::new();
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }

        let (start_addr, end_addr) = if let Some((start_raw, end_raw)) = raw.split_once('-') {
            (
                parse_addr_arg(start_raw, flag)?,
                parse_addr_arg(end_raw, flag)?,
            )
        } else {
            let addr = parse_addr_arg(raw, flag)?;
            (addr, addr)
        };

        if start_addr > end_addr {
            anyhow::bail!("{flag} range start must be <= end");
        }

        filters.push(HeadlessBusTraceFilter {
            start_addr,
            end_addr,
            access,
        });
    }

    if filters.is_empty() {
        anyhow::bail!("{flag} did not contain any address ranges");
    }

    Ok(filters)
}
