use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_ws_core::emulator::Emulator;
use zeff_ws_core::emulator::link_pair::{Endpoint, WonderSwanLinkPair, WonderSwanPairSnapshot};

use super::WsBackend;

#[derive(Clone, Copy)]
pub(crate) struct WsNetplayLoadProvenance {
    pub(crate) source: [u8; 32],
    pub(crate) source_len: u64,
    pub(crate) authenticated_source: bool,
    pub(crate) unmodified: bool,
    pub(crate) neutral_input: bool,
    pub(crate) state: [u8; 32],
    pub(crate) runtime: [u8; 32],
    pub(crate) persistent: [u8; 32],
    pub(crate) sample_rate: u32,
    pub(crate) config: [u8; 32],
    pub(crate) pair_checksum: [u8; 32],
}

pub(crate) struct WsBackendRollbackSession {
    pair: WonderSwanLinkPair,
    peer: Emulator,
    local_endpoint: Endpoint,
    audio_checksum: [u8; 32],
}

pub(crate) struct WsBackendRollbackSnapshot {
    core: WonderSwanPairSnapshot,
    local_endpoint: Endpoint,
    video_checksum: [u8; 32],
    audio_checksum: [u8; 32],
    persistent_checksum: [u8; 32],
}

impl WsBackendRollbackSnapshot {
    pub(crate) fn frame(&self) -> u64 {
        self.core.frame()
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.core.retained_bytes()
    }

    pub(crate) fn shared_media_bytes(&self) -> usize {
        self.core.shared_media_bytes()
    }

    pub(crate) fn checksum(&self) -> [u8; 32] {
        self.core.checksum()
    }

    pub(crate) fn video_checksum(&self) -> [u8; 32] {
        self.video_checksum
    }

    pub(crate) fn audio_checksum(&self) -> [u8; 32] {
        self.audio_checksum
    }

    pub(crate) fn persistent_checksum(&self) -> [u8; 32] {
        self.persistent_checksum
    }

    pub(crate) fn native_state(&self) -> Result<Vec<u8>> {
        self.core.native_state(self.local_endpoint)
    }
}

impl WsBackend {
    pub(crate) fn begin_netplay_rollback(
        &self,
        local_endpoint: Endpoint,
    ) -> Result<WsBackendRollbackSession> {
        let peer = self.emu.clone_for_link_peer();
        let machines = canonical_machines(&self.emu, &peer, local_endpoint);
        let pair = WonderSwanLinkPair::new(machines)?;
        Ok(WsBackendRollbackSession {
            pair,
            peer,
            local_endpoint,
            audio_checksum: hash_audio([&[], &[]]),
        })
    }

    pub(crate) fn validate_netplay_boundary(&self) -> Result<()> {
        self.netplay_initial_pair_checksum().map(|_| ())
    }

    pub(crate) fn netplay_initial_pair_checksum(&self) -> Result<[u8; 32]> {
        let peer = self.emu.clone_for_link_peer();
        let pair = WonderSwanLinkPair::new([&self.emu, &peer])?;
        Ok(pair.capture([&self.emu, &peer])?.checksum())
    }

    pub(crate) fn netplay_runtime_state_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.netplay_initial_pair_checksum()?.to_vec())
    }

    pub(crate) fn netplay_persistent_state_bytes(&self) -> Vec<u8> {
        persistent_bytes(&self.emu)
    }

    pub(crate) fn netplay_initial_persistent_checksum(&self) -> [u8; 32] {
        let bytes = self.netplay_persistent_state_bytes();
        hash_persistent([&bytes, &bytes])
    }

    pub(crate) fn netplay_config_bytes(&self) -> Vec<u8> {
        format!(
            "WS11:paired:48000:{:?}:{:?}:{}:{:?}",
            self.emu.footer().minimum_system,
            self.emu.footer().save_kind,
            self.emu.footer().rtc_present,
            self.emu.preferred_orientation(),
        )
        .into_bytes()
    }

    pub(crate) fn capture_netplay_load_provenance(
        &mut self,
        source: [u8; 32],
        source_len: usize,
        authenticated_source: bool,
        unmodified: bool,
        neutral_input: bool,
    ) -> Result<()> {
        self.netplay_load_provenance = None;
        let Ok(pair_checksum) = self.netplay_initial_pair_checksum() else {
            return Ok(());
        };
        let extension = self
            .paths
            .rom_path()
            .extension()
            .and_then(|ext| ext.to_str());
        self.netplay_load_provenance = Some(WsNetplayLoadProvenance {
            source,
            source_len: source_len as u64,
            authenticated_source: authenticated_source
                && extension.is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("ws") || ext.eq_ignore_ascii_case("wsc")
                }),
            unmodified,
            neutral_input,
            state: zeff_firmware::sha256_bytes(&self.emu.encode_state()?),
            runtime: zeff_firmware::sha256_bytes(&pair_checksum),
            persistent: self.netplay_initial_persistent_checksum(),
            sample_rate: self.emu.sample_rate(),
            config: zeff_firmware::sha256_bytes(&self.netplay_config_bytes()),
            pair_checksum,
        });
        Ok(())
    }

    pub(crate) fn netplay_load_provenance(&self) -> Option<WsNetplayLoadProvenance> {
        self.netplay_load_provenance
    }

    pub(crate) fn host_persistence_enabled(&self) -> bool {
        self.host_persistence_enabled
    }

    pub(crate) fn set_host_persistence_enabled(&mut self, enabled: bool) {
        self.host_persistence_enabled = enabled;
    }
}

impl WsBackendRollbackSession {
    pub(crate) fn capture(&self, backend: &WsBackend) -> Result<WsBackendRollbackSnapshot> {
        let machines = canonical_machines(&backend.emu, &self.peer, self.local_endpoint);
        Ok(WsBackendRollbackSnapshot {
            core: self.pair.capture(machines)?,
            local_endpoint: self.local_endpoint,
            video_checksum: self.pair.video_checksum(machines)?,
            audio_checksum: self.audio_checksum,
            persistent_checksum: hash_persistent([
                &persistent_bytes(machines[0]),
                &persistent_bytes(machines[1]),
            ]),
        })
    }

    pub(crate) fn restore(
        &mut self,
        backend: &mut WsBackend,
        snapshot: &WsBackendRollbackSnapshot,
    ) -> Result<()> {
        ensure!(
            snapshot.local_endpoint == self.local_endpoint,
            "paired local endpoint differs"
        );
        let machines =
            canonical_machines_mut(&mut backend.emu, &mut self.peer, self.local_endpoint);
        self.pair.restore(machines, &snapshot.core)?;
        self.audio_checksum = snapshot.audio_checksum;
        Ok(())
    }

    pub(crate) fn restore_after_session(
        &mut self,
        backend: &mut WsBackend,
        snapshot: &WsBackendRollbackSnapshot,
        checkpoint: &[u8],
    ) -> Result<()> {
        ensure!(
            checkpoint == snapshot.native_state()?,
            "WonderSwan restoration checkpoint differs"
        );
        self.restore(backend, snapshot)
    }

    pub(crate) fn advance_frame(
        &mut self,
        backend: &mut WsBackend,
        ports: [u16; 2],
    ) -> Result<Vec<f32>> {
        let machines =
            canonical_machines_mut(&mut backend.emu, &mut self.peer, self.local_endpoint);
        let output = self.pair.advance_frame(machines, ports)?;
        self.audio_checksum = hash_audio([&output.audio[0], &output.audio[1]]);
        let [zero, one] = output.audio;
        Ok(match self.local_endpoint {
            Endpoint::Zero => zero,
            Endpoint::One => one,
        })
    }
}

fn canonical_machines<'a>(
    local: &'a Emulator,
    peer: &'a Emulator,
    endpoint: Endpoint,
) -> [&'a Emulator; 2] {
    match endpoint {
        Endpoint::Zero => [local, peer],
        Endpoint::One => [peer, local],
    }
}

fn canonical_machines_mut<'a>(
    local: &'a mut Emulator,
    peer: &'a mut Emulator,
    endpoint: Endpoint,
) -> [&'a mut Emulator; 2] {
    match endpoint {
        Endpoint::Zero => [local, peer],
        Endpoint::One => [peer, local],
    }
}

fn hash_audio(audio: [&[f32]; 2]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"zeff-ws-pair-audio-v1\0");
    for samples in audio {
        hash.update((samples.len() as u64).to_le_bytes());
        for sample in samples {
            hash.update(sample.to_bits().to_le_bytes());
        }
    }
    hash.finalize().into()
}

fn persistent_bytes(machine: &Emulator) -> Vec<u8> {
    if machine.footer().rtc_present {
        machine.dump_complete_rtc_persistence().unwrap_or_default()
    } else {
        machine.dump_battery_sram().unwrap_or_default()
    }
}

fn hash_persistent(persistent: [&[u8]; 2]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"zeff-ws-pair-persistent-v1\0");
    for bytes in persistent {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

#[cfg(test)]
pub(crate) mod tests;
