use std::cell::Cell;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::prelude::*;
use zeff_netplay::rollback::InputDelay;
use zeff_netplay_connect::protocol::SessionIdentity;

use super::*;
use crate::netplay::connect::{Connector, executable_build, lobby};
use crate::netplay::qualification::{Frame, Progress, input, media, ports};
use crate::netplay::{compatibility, identity};
use crate::platform::Instant;

mod observation;
mod storage;
use observation::Observation;

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

struct Active;

impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Options {
    timing: u8,
    delay: u64,
    native_host: bool,
    frames: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rom: Option<Vec<u8>>,
}

struct Restoration<'a> {
    native: &'a [u8],
    runtime: &'a [u8],
    persistent: &'a [u8],
}

#[wasm_bindgen(inline_js = r#"
export function qualificationYield() { return new Promise(resolve => setTimeout(resolve, 1)); }
export async function qualificationRequest(path, body) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 3000);
    try {
        const response = await fetch('http://127.0.0.1:47180/test/gameplay/' + path,
            {method: body == null ? 'GET' : 'POST', body: body ?? undefined,
             cache: 'no-store', signal: controller.signal});
        if (!response.ok || !response.body) throw new Error('qualification HTTP ' + response.status);
        const reader = response.body.getReader();
        const data = new Uint8Array(4194304);
        let length = 0;
        while (true) {
            const {done, value} = await reader.read();
            if (done) break;
            if (value.length > data.length - length) throw new Error('qualification response too large');
            data.set(value, length); length += value.length;
        }
        return new TextDecoder().decode(data.subarray(0, length));
    } finally { clearTimeout(timer); controller.abort(); }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = qualificationYield)]
    async fn qualification_yield();
    #[wasm_bindgen(catch, js_name = qualificationRequest)]
    async fn qualification_request(path: &str, body: Option<&str>) -> Result<JsValue, JsValue>;
}

async fn http(path: &str, body: Option<&str>) -> Result<String> {
    qualification_request(path, body)
        .await
        .map_err(|error| anyhow::anyhow!("qualification request failed: {error:?}"))?
        .as_string()
        .context("qualification response is not text")
}

async fn status() -> Result<Progress> {
    let result: Progress = serde_json::from_str(&http("status", None).await?)?;
    ensure!(result.error.is_none(), "native failure: {:?}", result.error);
    Ok(result)
}

#[wasm_bindgen(js_name = zeffBoyQualifyNetplay)]
pub async fn qualify(options_json: &str) -> Result<String, JsValue> {
    run(options_json)
        .await
        .map_err(|error| JsValue::from_str(&format!("{error:#}")))
}

#[wasm_bindgen(js_name = zeffBoyQualificationFacts)]
pub fn facts() -> Result<String, JsValue> {
    let result = (|| -> Result<String> {
        let mut facts = crate::netplay::qualification::facts(executable_build()?);
        facts["certificate"] = serde_json::to_value(compatibility::certificate_status()?)?;
        Ok(serde_json::to_string(&facts)?)
    })();
    result.map_err(|error| JsValue::from_str(&format!("{error:#}")))
}

async fn run(options_json: &str) -> Result<String> {
    ensure!(
        options_json.len() <= 16 * 1024 * 1024,
        "qualification options too large"
    );
    let location = js_sys::Reflect::get(&js_sys::global(), &"location".into())
        .map_err(|error| anyhow::anyhow!("browser location unavailable: {error:?}"))?;
    let origin = js_sys::Reflect::get(&location, &"origin".into())
        .map_err(|error| anyhow::anyhow!("browser origin unavailable: {error:?}"))?
        .as_string()
        .context("browser origin unavailable")?;
    ensure!(
        origin == "http://127.0.0.1:47181",
        "qualification requires the owned loopback origin"
    );
    ensure!(
        ACTIVE.with(|active| !active.replace(true)),
        "qualification already active"
    );
    let _active = Active;
    let options: Options = serde_json::from_str(options_json)?;
    ensure!([0, 1, 3].contains(&options.timing), "unsupported timing");
    ensure!([0, 2].contains(&options.delay), "unsupported delay");
    ensure!(
        (72..=1000).contains(&options.frames),
        "unsupported frame count"
    );
    ensure!(
        options
            .rom
            .as_ref()
            .is_none_or(|rom| (16..=1024 * 1024).contains(&rom.len()) && &rom[..4] == b"NES\x1a"),
        "invalid qualification ROM"
    );
    let native_started = Cell::new(false);
    let result = run_owned(options, &native_started).await;
    if result.is_err() && native_started.get() {
        let _ = http("control", Some("cancel")).await;
    }
    result
}

async fn run_owned(options: Options, native_started: &Cell<bool>) -> Result<String> {
    crate::platform::init_storage().await;
    let bytes = options.rom.clone().unwrap_or_else(|| media(options.timing));
    let persistent = storage::prepare(&bytes, options.rom.is_some()).await?;
    let local = storage::backend(&bytes)?;
    ensure!(
        local
            .nes()
            .context("NES required")?
            .emu
            .dump_persistent_data()
            .unwrap_or_default()
            == persistent,
        "loaded browser persistent seed differs"
    );
    let original = local.encode_state_bytes()?;
    let original_runtime = local
        .nes()
        .context("NES required")?
        .emu
        .encode_rollback_runtime_state();
    let build = executable_build()?;
    let delay = InputDelay::new(options.delay)?;
    let id = identity::identity_with_delay(&local, build, delay)?;
    ensure!(
        id.build_info.platform == 3 && id.build_info.certificate != [0; 32],
        "a signed matching browser pair is required"
    );
    let certificate = compatibility::certificate_status()?.context("pair certificate absent")?;
    let session = SessionIdentity {
        core: "nes".into(),
        content_hash: const_hex::encode(id.effective),
        compatibility_hash: const_hex::encode(id.config),
        mode: crate::netplay::capabilities::session_mode(crate::emu_backend::ActiveSystem::Nes),
    };
    let lobby_options = lobby::Options {
        url: "ws://127.0.0.1:47180/v1/ws".into(),
        access_token: String::new(),
        input_delay: delay,
    };
    let mut connector = if options.native_host {
        let invitation = start_native(&options, "").await?;
        native_started.set(true);
        Connector::lobby_join(lobby_options, &invitation, session)?
    } else {
        let mut connector = Connector::lobby_host(lobby_options, session)?;
        let waiting = Instant::now();
        let invitation = loop {
            if let Some(invitation) = connector.take_invitation() {
                break invitation;
            }
            ensure!(
                waiting.elapsed() < Duration::from_secs(5),
                "browser invitation deadline"
            );
            qualification_yield().await;
        };
        let result = start_native(&options, &invitation).await?;
        native_started.set(true);
        ensure!(result == invitation, "native invitation differs");
        connector
    };
    let waiting = Instant::now();
    let mut start = loop {
        if let Some(start) = connector.poll()? {
            break start;
        }
        ensure!(
            waiting.elapsed() < Duration::from_secs(10),
            "browser connection deadline"
        );
        qualification_yield().await;
    };
    ensure!(
        start.build == build && !start.allow_different_versions,
        "connector facts differ"
    );
    start.verify_every_frame = true;
    let thread = EmuThread::spawn(local, false);
    thread.send(EmuCommand::StartNetplay(Box::new(start)));
    let result = execute(
        &options,
        &thread,
        &bytes,
        id.config,
        Restoration {
            native: &original,
            runtime: &original_runtime,
            persistent: &persistent,
        },
    )
    .await;
    if result.is_err() {
        thread.send(EmuCommand::StopNetplay);
        let _ = http("control", Some("stop")).await;
        let _ = storage::shutdown(&thread, &bytes, &persistent).await;
    }
    let (native, mut browser) = result?;
    browser.facts["game_source"] = json!(const_hex::encode(id.source));
    Ok(serde_json::to_string(&json!({
        "production_browser": true, "test": cfg!(test),
        "source_sha256": const_hex::encode(id.source),
        "options":{"timing":options.timing,"delay":options.delay,
            "native_host":options.native_host,"frames":options.frames},
        "build_sha256": const_hex::encode(build), "app_version": env!("CARGO_PKG_VERSION"),
        "compiled_full_source": env!("ZEFF_NETPLAY_FULL_SOURCE"),
        "compiled_qualification_source": env!("ZEFF_NETPLAY_QUALIFICATION_SOURCE"),
        "compiled_target": env!("ZEFF_NETPLAY_BUILD_TARGET"),
        "compiled_profile": env!("ZEFF_NETPLAY_BUILD_PROFILE"),
        "compiled_opt_level": env!("ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        "compiled_debug": env!("ZEFF_NETPLAY_BUILD_DEBUG"),
        "compiled_compiler": env!("ZEFF_NETPLAY_BUILD_COMPILER"),
        "compiled_target_features": env!("ZEFF_NETPLAY_BUILD_FEATURES"),
        "certificate": certificate, "native": native, "browser": browser,
    }))?)
}

async fn start_native(options: &Options, invitation: &str) -> Result<String> {
    let body = json!({"timing": options.timing, "delay": options.delay,
        "host": options.native_host, "room": invitation, "frames": options.frames,
        "rom": options.rom});
    http("start", Some(&serde_json::to_string(&body)?)).await
}

async fn execute(
    options: &Options,
    thread: &EmuThread,
    bytes: &[u8],
    config: [u8; 32],
    restoration: Restoration<'_>,
) -> Result<(Progress, Progress)> {
    let mut observed = Observation::default();
    let started = Instant::now();
    loop {
        observed.drain(thread, options.frames + 24)?;
        ensure!(
            !observed.stopped,
            "browser stopped before play: {:?}",
            observed.stop_reason
        );
        let native = status().await?;
        if observed.ready && native.presented >= 6 {
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(10),
            "prediction setup deadline"
        );
        qualification_yield().await;
    }
    http("control", Some("release")).await?;
    let native = loop {
        observed.drain(thread, options.frames + 24)?;
        ensure!(!observed.stopped, "browser stopped during play");
        ensure!(
            observed.presented < (options.frames + 24) as u64,
            "browser presentation overflow"
        );
        if !observed.pending && observed.frames.len() < options.frames {
            thread.send(EmuCommand::StepNetplay(input(
                observed.presented,
                usize::from(options.native_host),
            )));
            observed.pending = true;
        }
        let native = status().await?;
        if observed.frames.len() >= options.frames
            && native.verified >= options.frames
            && !native.pending
            && !observed.pending
        {
            break native;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(60),
            "gameplay deadline"
        );
        qualification_yield().await;
    };
    ensure!(native.replayed > 0, "native prediction was not corrected");
    let close_frame = thread.inner.borrow().backend.frame_count();
    if options.native_host {
        http("control", Some("stop")).await?;
    } else {
        thread.send(EmuCommand::StopNetplay);
    }
    let stopped = Instant::now();
    while !observed.stopped || crate::netplay::network::Network::pending_closes() != 0 {
        observed.drain(thread, options.frames + 24)?;
        ensure!(
            stopped.elapsed() < Duration::from_secs(2),
            "browser stop deadline"
        );
        qualification_yield().await;
    }
    let receipts = verify_reference(options, bytes, config, &observed).await?;
    storage::check_restored(
        thread,
        restoration.native,
        restoration.runtime,
        restoration.persistent,
    )?;
    let native = loop {
        let native = status().await?;
        if native.finished {
            break native;
        }
        ensure!(
            stopped.elapsed() < Duration::from_secs(10),
            "native shutdown deadline"
        );
        qualification_yield().await;
    };
    ensure!(
        native.restored && native.published,
        "native restore/publication differs"
    );
    if options.native_host {
        ensure!(
            native.stop_reason.as_deref() == Some("local stop"),
            "native local stop reason differs"
        );
        ensure!(
            observed.stop_reason.as_deref()
                == Some(format!("netplay peer closed at frame {}", native.closed_at).as_str()),
            "browser peer close differs"
        );
    } else {
        ensure!(
            observed.stop_reason.as_deref() == Some("Disconnected"),
            "browser local stop reason differs"
        );
        ensure!(
            native.stop_reason.as_deref()
                == Some(format!("netplay peer closed at frame {close_frame}").as_str()),
            "native peer close differs"
        );
    }
    ensure!(
        native.frames == receipts[..options.frames],
        "peer confirmed receipts differ"
    );
    storage::shutdown(thread, bytes, restoration.persistent).await?;
    let browser = Progress {
        ready: observed.ready,
        presented: observed.presented,
        pending: observed.pending,
        verified: receipts.len(),
        replayed: observed.replayed,
        restored: true,
        published: true,
        closed_at: close_frame,
        stop_reason: observed.stop_reason,
        finished: true,
        frames: receipts,
        facts: crate::netplay::qualification::facts(executable_build()?),
        ..Default::default()
    };
    Ok((native, browser))
}

async fn verify_reference(
    options: &Options,
    bytes: &[u8],
    config: [u8; 32],
    observed: &Observation,
) -> Result<Vec<Frame>> {
    let mut reference = storage::backend(bytes)?;
    let crate::emu_backend::EmuBackend::Nes(nes) = &mut reference else {
        anyhow::bail!("NES required");
    };
    let lease = nes.emu.begin_rollback_session()?;
    let mut receipts = Vec::with_capacity(observed.frames.len());
    for (index, (checkpoint, actual_ports, audio)) in observed.frames.iter().enumerate() {
        let ports = ports(index as u64, options.delay);
        let crate::emu_backend::EmuBackend::Nes(nes) = &mut reference else {
            anyhow::bail!("NES required");
        };
        let pcm = lease.advance_frame(&mut nes.emu, ports.map(|port| port as u8))?;
        let expected = identity::checkpoint(&reference, index as u64 + 1, &pcm, config)?;
        ensure!(
            *checkpoint == expected && *actual_ports == ports,
            "browser checkpoint/ports differ at {index}"
        );
        ensure!(
            audio.iter().all(|sample| sample.is_finite())
                && audio
                    .iter()
                    .map(|sample| sample.to_bits())
                    .eq(pcm.iter().map(|sample| sample.to_bits())),
            "browser PCM differs at {index}"
        );
        receipts.push(Frame::new(&expected, ports, &pcm)?);
        qualification_yield().await;
    }
    Ok(receipts)
}
