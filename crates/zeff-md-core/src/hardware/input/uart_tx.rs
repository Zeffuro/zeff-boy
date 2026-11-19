use super::uart_idle::OUTPUT_IDLE_SETTLED_TICKS;
use crate::Region;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Transfer {
    written: u64,
    load: u64,
    value: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Port {
    qualified: bool,
    receive_high: bool,
    last_load: Option<u64>,
    transfer: Option<Transfer>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UartTx {
    cold_profile: bool,
    pal: Option<bool>,
    ports: [Port; 2],
}

#[derive(Clone, Copy)]
struct Grid {
    rx: u64,
    tx: u64,
    base: u64,
    period: u64,
}

impl Grid {
    fn new(pal: bool) -> Self {
        if pal {
            Self {
                rx: 242,
                tx: 5541,
                base: 693,
                period: 11088,
            }
        } else {
            Self {
                rx: 249,
                tx: 5597,
                base: 700,
                period: 11200,
            }
        }
    }

    fn after(first: u64, period: u64, at: u64) -> Option<u64> {
        if at < first {
            Some(first)
        } else {
            let steps = ((at - first) / period).checked_add(1)?;
            first.checked_add(steps.checked_mul(period)?)
        }
    }

    fn opening(self, written: u64) -> Option<u64> {
        let opening = written.checked_sub(14)?;
        let on_rx = opening >= self.rx && (opening - self.rx).is_multiple_of(self.base);
        (!on_rx && !self.on_tx(opening)).then_some(opening)
    }

    fn on_tx(self, at: u64) -> bool {
        at >= self.tx && (at - self.tx).is_multiple_of(self.period)
    }

    fn load(self, written: u64, last: Option<u64>) -> Option<u64> {
        // RX may sample inside the pulse, which opens 14 ticks before service.
        let opening = self.opening(written)?;
        let sample = Self::after(self.rx, self.base, opening)?;
        let eligible = Self::after(self.tx, self.period, sample)?;
        let available = match last {
            Some(last) => last.checked_add(self.period.checked_mul(11)?)?,
            None => 0,
        };
        Some(eligible.max(available))
    }
}

impl Port {
    fn last_at(self, at: u64) -> Option<u64> {
        self.transfer
            .filter(|transfer| transfer.load <= at)
            .map(|transfer| transfer.load)
            .or(self.last_load)
    }

    fn full(self, at: u64) -> bool {
        self.transfer.is_some_and(|transfer| transfer.load > at)
    }
}

impl UartTx {
    pub fn cold() -> Self {
        // One software cold endpoint; both ports share its absolute clock grid.
        Self {
            cold_profile: true,
            pal: None,
            ports: [Port {
                qualified: true,
                ..Port::default()
            }; 2],
        }
    }

    fn grid(&self, region: Region) -> Option<Grid> {
        let pal = region == Region::Pal;
        (self.cold_profile && self.pal.is_none_or(|saved| saved == pal)).then(|| Grid::new(pal))
    }

    pub fn used(&self, port: usize) -> bool {
        self.ports
            .get(port)
            .is_some_and(|port| port.transfer.is_some())
    }

    pub fn any_used(&self) -> bool {
        (0..2).any(|port| self.used(port))
    }

    pub fn receive_high(&self, port: usize) -> bool {
        self.ports.get(port).is_some_and(|port| port.receive_high)
    }

    pub fn retire_port(&mut self, port: usize) {
        if let Some(state) = self.ports.get_mut(port)
            && state.transfer.is_none()
        {
            *state = Port::default();
        }
    }

    pub fn retire(&mut self) {
        *self = Self::default();
    }

    pub fn status(&self, port: usize, begin: u64, end: u64, region: Region) -> Option<u8> {
        self.grid(region)?;
        let state = *self.ports.get(port)?;
        // S7 is E-3 and samples before the same-tick derived TX update.
        let sample = end.checked_sub(3)?;
        if !state.qualified
            || end.checked_sub(begin).is_none_or(|ticks| ticks < 28)
            || state
                .transfer
                .is_some_and(|transfer| begin < transfer.written)
        {
            return None;
        }
        Some(u8::from(
            state
                .transfer
                .is_some_and(|transfer| sample <= transfer.load),
        ))
    }

    pub fn transmit(
        &mut self,
        port: usize,
        value: u8,
        begin: u64,
        end: u64,
        region: Region,
    ) -> bool {
        let Some(grid) = self.grid(region) else {
            return false;
        };
        let Some(state) = self.ports.get(port).copied() else {
            return false;
        };
        if !state.qualified
            || begin < OUTPUT_IDLE_SETTLED_TICKS
            || end.checked_sub(begin).is_none_or(|ticks| ticks < 28)
            || state.full(begin)
            || state
                .transfer
                .is_some_and(|transfer| begin < transfer.written)
        {
            return false;
        }
        let last_load = state.last_at(begin);
        let Some(load) = grid.load(end, last_load) else {
            return false;
        };
        self.pal = Some(region == Region::Pal);
        self.ports[port] = Port {
            qualified: true,
            receive_high: true,
            last_load,
            transfer: Some(Transfer {
                written: end,
                load,
                value,
            }),
        };
        true
    }

    pub fn latch_matches(&self, port: usize, value: u8) -> bool {
        self.ports
            .get(port)
            .and_then(|port| port.transfer)
            .is_none_or(|transfer| transfer.value == value)
    }

    pub fn validate(&self, now: u64, region: Option<Region>) -> Result<()> {
        ensure!(
            self.cold_profile || self == &Self::default(),
            "invalid MD UART cold profile"
        );
        ensure!(
            self.pal.is_none() || self.any_used(),
            "invalid MD UART region owner"
        );
        if let Some(region) = region {
            ensure!(
                self.pal.is_none_or(|pal| pal == (region == Region::Pal)),
                "MD UART clock region mismatch"
            );
        }
        for state in self.ports {
            ensure!(
                state.qualified || state == Port::default(),
                "invalid MD UART retired port"
            );
            ensure!(
                state.receive_high == state.transfer.is_some(),
                "invalid MD UART RX provenance"
            );
            ensure!(
                state.transfer.is_some() || state.last_load.is_none(),
                "invalid MD UART load history"
            );
            if let Some(transfer) = state.transfer {
                let grid = Grid::new(
                    self.pal
                        .ok_or_else(|| anyhow::anyhow!("missing MD UART region"))?,
                );
                ensure!(
                    transfer.written <= now && transfer.written >= OUTPUT_IDLE_SETTLED_TICKS + 28,
                    "invalid MD UART write time"
                );
                ensure!(
                    grid.opening(transfer.written).is_some(),
                    "invalid MD UART write opening"
                );
                let latest_prior = transfer
                    .written
                    .checked_sub(28)
                    .ok_or_else(|| anyhow::anyhow!("invalid MD UART write envelope"))?;
                let earliest = grid
                    .load(OUTPUT_IDLE_SETTLED_TICKS + 28, None)
                    .ok_or_else(|| anyhow::anyhow!("invalid MD UART cold schedule"))?;
                ensure!(
                    state.last_load.is_none_or(|last| last >= earliest
                        && last <= latest_prior
                        && grid.on_tx(last)),
                    "invalid MD UART prior load"
                );
                ensure!(
                    grid.load(transfer.written, state.last_load) == Some(transfer.load),
                    "invalid MD UART pending load"
                );
            }
        }
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.cold_profile);
        writer.write_u8(match self.pal {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        for state in self.ports {
            writer.write_bool(state.qualified);
            writer.write_bool(state.receive_high);
            writer.write_bool(state.last_load.is_some());
            if let Some(last) = state.last_load {
                writer.write_u64(last);
            }
            writer.write_bool(state.transfer.is_some());
            if let Some(transfer) = state.transfer {
                writer.write_u64(transfer.written);
                writer.write_u64(transfer.load);
                writer.write_u8(transfer.value);
            }
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut result = Self {
            cold_profile: reader.read_bool()?,
            pal: match reader.read_u8()? {
                0 => None,
                1 => Some(false),
                2 => Some(true),
                _ => anyhow::bail!("invalid MD UART clock region"),
            },
            ports: [Port::default(); 2],
        };
        for state in &mut result.ports {
            state.qualified = reader.read_bool()?;
            state.receive_high = reader.read_bool()?;
            state.last_load = if reader.read_bool()? {
                Some(reader.read_u64()?)
            } else {
                None
            };
            state.transfer = if reader.read_bool()? {
                Some(Transfer {
                    written: reader.read_u64()?,
                    load: reader.read_u64()?,
                    value: reader.read_u8()?,
                })
            } else {
                None
            };
        }
        Ok(result)
    }
}
