use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
#[cfg(target_arch = "wasm32")]
use std::path::Path;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use zeff_nes_core::emulator::rollback::NesRollbackSession;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Options {
    timing: u8,
    frames: usize,
}

pub(crate) struct Quiet {
    #[cfg(not(target_arch = "wasm32"))]
    _media_root: tempfile::TempDir,
    backend: EmuBackend,
    lease: NesRollbackSession,
    original: Vec<u8>,
    runtime: Vec<u8>,
    snapshot: zeff_nes_core::emulator::rollback::NesRollbackSnapshot,
    tail_snapshot: Option<zeff_nes_core::emulator::rollback::NesRollbackSnapshot>,
    tail_state: Vec<u8>,
    tail_runtime: Vec<u8>,
    config: [u8; 32],
    build: [u8; 32],
    frames: usize,
    timing: u8,
    trace: Vec<super::Frame>,
    subnormals: usize,
    peak: f32,
    tail_peak: f32,
}

pub(crate) fn options(text: &str) -> Result<Options> {
    ensure!(text.len() <= 1024, "quiet options exceed limit");
    let options: Options = serde_json::from_str(text)?;
    ensure!(
        [0, 1, 3].contains(&options.timing) && (300..=1000).contains(&options.frames),
        "unsupported quiet case"
    );
    Ok(options)
}

fn quiet_media(timing: u8) -> Vec<u8> {
    let mut bytes = super::media(timing);
    let sounding = [
        0xa5, 0x00, 0x09, 0x20, 0x8d, 0x02, 0x40, 0xa5, 0x01, 0x09, 0x40, 0x8d, 0x06, 0x40,
    ];
    let position = bytes
        .windows(sounding.len())
        .position(|window| window == sounding)
        .unwrap();
    bytes[position..position + sounding.len()].fill(0xea);
    bytes[position..position + 3].copy_from_slice(&[0x20, 0x00, 0x82]);
    // The final 19-cycle gap leaves the triangle DAC at zero for both reset parities.
    let routine = [
        0xa5, 0x02, 0xc9, 0x08, 0x90, 0x01, 0x60, 0xe6, 0x02, 0xc9, 0x07, 0xf0, 0x0b, 0xa9, 0x20,
        0x8d, 0x02, 0x40, 0xa9, 0x40, 0x8d, 0x06, 0x40, 0x60, 0xa9, 0xc0, 0x8d, 0x17, 0x40, 0xa9,
        0xff, 0x8d, 0x08, 0x40, 0xa9, 0x00, 0x8d, 0x0a, 0x40, 0xa9, 0x04, 0x8d, 0x15, 0x40, 0xa9,
        0xf8, 0x8d, 0x0b, 0x40, 0xa9, 0xc0, 0x8d, 0x17, 0x40, 0xea, 0xea, 0xea, 0xea, 0xea, 0x24,
        0x02, 0xa9, 0x00, 0x8d, 0x15, 0x40, 0x60,
    ];
    bytes[16 + 0x200..16 + 0x200 + routine.len()].copy_from_slice(&routine);
    bytes[16 + 0x300..16 + 0x307].copy_from_slice(&[0xa9, 0x00, 0x85, 0x02, 0x4c, 0x00, 0x80]);
    bytes[16 + 0x7ffc..16 + 0x7ffe].copy_from_slice(&[0x00, 0x83]);
    bytes
}

impl Quiet {
    pub(crate) fn new(options: Options) -> Result<Self> {
        let bytes = quiet_media(options.timing);
        #[cfg(target_arch = "wasm32")]
        let path = Path::new("artifact-quiet-qualification.nes");
        #[cfg(not(target_arch = "wasm32"))]
        let media_root = tempfile::tempdir()?;
        #[cfg(not(target_arch = "wasm32"))]
        let media_path = media_root.path().join("artifact-quiet-qualification.nes");
        #[cfg(not(target_arch = "wasm32"))]
        std::fs::write(&media_path, &bytes)?;
        #[cfg(not(target_arch = "wasm32"))]
        let path = media_path.as_path();
        #[allow(unused_mut)]
        let mut config = BackendLoadConfig {
            sample_rate: Some(48_000),
            ..Default::default()
        };
        #[cfg(target_arch = "wasm32")]
        {
            config.netplay_browser_media =
                crate::emu_backend::loader::NetplayRomMedia::browser(&bytes);
        }
        #[cfg(target_arch = "wasm32")]
        let supplied_bytes = Some(bytes);
        #[cfg(not(target_arch = "wasm32"))]
        let supplied_bytes = None;
        let mut backend =
            load_backend_from_rom_source(ActiveSystem::Nes, path, path, supplied_bytes, config)?
                .backend;
        let original = backend.encode_state_bytes()?;
        let runtime = core(&mut backend).encode_rollback_runtime_state();
        let build = crate::netplay::connect::executable_build()?;
        let config = crate::netplay::identity::identity(&backend, build)?.config;
        let lease = core(&mut backend).begin_rollback_session()?;
        let snapshot = lease.capture(core(&mut backend))?;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            _media_root: media_root,
            backend,
            lease,
            original,
            runtime,
            snapshot,
            tail_snapshot: None,
            tail_state: Vec::new(),
            tail_runtime: Vec::new(),
            config,
            build,
            frames: options.frames,
            timing: options.timing,
            trace: Vec::new(),
            subnormals: 0,
            peak: 0.0,
            tail_peak: 0.0,
        })
    }

    pub(crate) fn step(&mut self) -> Result<bool> {
        if self.trace.len() == self.frames {
            return Ok(false);
        }
        if self.trace.len() == self.frames - 32 {
            self.tail_state = self.backend.encode_state_bytes()?;
            self.tail_runtime = core(&mut self.backend).encode_rollback_runtime_state();
            self.tail_snapshot = Some(self.lease.capture(core(&mut self.backend))?);
        }
        let pcm = self.lease.advance_frame(core(&mut self.backend), [0; 2])?;
        ensure!(
            pcm.iter().all(|sample| sample.is_finite()),
            "nonfinite quiet PCM"
        );
        for sample in &pcm {
            self.subnormals += usize::from(sample.is_subnormal());
            self.peak = self.peak.max(sample.abs());
            if self.trace.len() + 1 == self.frames {
                self.tail_peak = self.tail_peak.max(sample.abs());
            }
        }
        let frame = self.trace.len() as u64 + 1;
        let checkpoint =
            crate::netplay::identity::checkpoint(&self.backend, frame, &pcm, self.config)?;
        self.trace
            .push(super::Frame::new(&checkpoint, [0; 2], &pcm)?);
        Ok(true)
    }

    pub(crate) fn finish(mut self) -> Result<Value> {
        ensure!(
            self.trace.len() == self.frames && self.peak > 0.0 && self.tail_peak < 0.00001,
            "quiet fixture did not exercise sounding then quiet output"
        );
        ensure!(
            self.subnormals > 0 && self.tail_peak.is_subnormal(),
            "quiet PCM lacks subnormal tail: count={}, peak={}",
            self.subnormals,
            self.tail_peak
        );
        let final_state = self.backend.encode_state_bytes()?;
        let final_runtime = core(&mut self.backend).encode_rollback_runtime_state();
        self.lease
            .restore(core(&mut self.backend), &self.snapshot)?;
        ensure!(
            self.backend.encode_state_bytes()? == self.original
                && core(&mut self.backend).encode_rollback_runtime_state() == self.runtime,
            "quiet continuation restoration differs"
        );
        for index in 0..32 {
            let pcm = self.lease.advance_frame(core(&mut self.backend), [0; 2])?;
            let checkpoint = crate::netplay::identity::checkpoint(
                &self.backend,
                index as u64 + 1,
                &pcm,
                self.config,
            )?;
            ensure!(
                super::Frame::new(&checkpoint, [0; 2], &pcm)? == self.trace[index],
                "quiet output continuation differs after restore"
            );
        }
        self.lease.restore(
            core(&mut self.backend),
            self.tail_snapshot.as_ref().unwrap(),
        )?;
        ensure!(
            self.backend.encode_state_bytes()? == self.tail_state
                && core(&mut self.backend).encode_rollback_runtime_state() == self.tail_runtime,
            "quiet tail restoration differs"
        );
        for index in self.frames - 32..self.frames {
            let pcm = self.lease.advance_frame(core(&mut self.backend), [0; 2])?;
            let checkpoint = crate::netplay::identity::checkpoint(
                &self.backend,
                index as u64 + 1,
                &pcm,
                self.config,
            )?;
            ensure!(
                super::Frame::new(&checkpoint, [0; 2], &pcm)? == self.trace[index],
                "quiet tail output continuation differs after restore"
            );
        }
        ensure!(
            self.backend.encode_state_bytes()? == final_state
                && core(&mut self.backend).encode_rollback_runtime_state() == final_runtime,
            "quiet final continuation differs"
        );
        Ok(json!({"finite":true, "restored":true, "trace":self.trace,
            "options":{"timing":self.timing,"frames":self.frames},
            "continuation_frames":32, "tail_continuation_frames":32,
            "subnormal_count":self.subnormals, "arithmetic": arithmetic(),
            "peak_bits":self.peak.to_bits(), "tail_peak_bits":self.tail_peak.to_bits(), "facts":super::facts(self.build)}))
    }
}

fn core(backend: &mut EmuBackend) -> &mut zeff_nes_core::emulator::Emulator {
    match backend {
        EmuBackend::Nes(nes) => &mut nes.emu,
        _ => unreachable!("quiet NES backend"),
    }
}

fn arithmetic() -> Value {
    let mut f32_value = std::hint::black_box(f32::MIN_POSITIVE);
    let mut f64_value = std::hint::black_box(f64::MIN_POSITIVE);
    let mut f32_bits = Vec::new();
    let mut f64_bits = Vec::new();
    let mut subnormals = 0;
    for _ in 0..64 {
        f32_value *= std::hint::black_box(0.5);
        f64_value *= std::hint::black_box(0.5);
        subnormals += usize::from(f32_value.is_subnormal()) + usize::from(f64_value.is_subnormal());
        f32_bits.push(f32_value.to_bits());
        f64_bits.push(f64_value.to_bits());
    }
    json!({"f32":f32_bits, "f64":f64_bits, "subnormal_count":subnormals})
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = zeffBoyQualifyQuiet)]
pub async fn browser(text: &str) -> Result<String, wasm_bindgen::JsValue> {
    async fn execute(text: &str) -> Result<String> {
        let mut quiet = Quiet::new(options(text)?)?;
        while quiet.step()? {
            yield_quiet().await;
        }
        Ok(serde_json::to_string(&quiet.finish()?)?)
    }
    execute(text)
        .await
        .map_err(|error| wasm_bindgen::JsValue::from_str(&format!("{error:#}")))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(
    inline_js = "export function quietYield() { return new Promise(resolve => setTimeout(resolve, 0)); }"
)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = quietYield)]
    async fn yield_quiet();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_quiet_authenticates_media_and_restores_subnormal_continuation() {
        for timing in [0, 1, 3] {
            let mut quiet = Quiet::new(Options {
                timing,
                frames: 300,
            })
            .unwrap();
            while quiet.step().unwrap() {}
            let report = quiet.finish().unwrap();
            assert_eq!(report["restored"], true);
            assert!(report["subnormal_count"].as_u64().unwrap() > 0);
            let bits = report["tail_peak_bits"].as_u64().unwrap();
            assert!(bits > 0 && bits < 0x0080_0000);
        }
    }

    #[test]
    fn arithmetic_observes_gradual_underflow_and_quiet_options_are_bounded() {
        let result = arithmetic();
        assert_eq!(result["subnormal_count"], 75);
        assert_eq!(result["f32"][0], 0x0040_0000u32);
        assert_eq!(result["f32"][22], 1);
        assert_eq!(result["f32"][23], 0);
        assert_eq!(result["f64"][51], 1);
        assert_eq!(result["f64"][52], 0);
        assert!(options(r#"{"timing":3,"frames":300}"#).is_ok());
        for bad in [
            r#"{"timing":2,"frames":300}"#,
            r#"{"timing":0,"frames":299}"#,
            r#"{"timing":0,"frames":1001}"#,
            r#"{"timing":0,"frames":300,"unexpected":1}"#,
        ] {
            assert!(options(bad).is_err());
        }
    }
}
