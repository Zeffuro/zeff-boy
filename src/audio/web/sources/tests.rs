use super::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{AudioBuffer, OfflineAudioContext};

const RATE: f32 = 48_000.0;
const BUFFER: usize = 1024;

fn context(frames: u32) -> OfflineAudioContext {
    OfflineAudioContext::new_with_number_of_channels_and_length_and_sample_rate(2, frames, RATE)
        .unwrap()
}

fn source(ctx: &OfflineAudioContext, samples: [f32; 2]) -> AudioBufferSourceNode {
    let buffer = ctx.create_buffer(2, BUFFER as u32, RATE).unwrap();
    for (channel, sample) in samples.into_iter().enumerate() {
        buffer
            .copy_to_channel(&vec![sample; BUFFER], channel as i32)
            .unwrap();
    }
    let node = ctx.create_buffer_source().unwrap();
    node.set_buffer(Some(&buffer));
    node.connect_with_audio_node(&ctx.destination()).unwrap();
    node
}

async fn render(ctx: &OfflineAudioContext) -> AudioBuffer {
    JsFuture::from(ctx.start_rendering().unwrap())
        .await
        .unwrap()
        .dyn_into()
        .unwrap()
}

fn exact(buffer: &AudioBuffer, segments: &[(std::ops::Range<usize>, [f32; 2])]) {
    for channel in 0..2 {
        let pcm = buffer.get_channel_data(channel as u32).unwrap();
        for (frame, actual) in pcm.into_iter().enumerate() {
            let expected = segments
                .iter()
                .find(|(range, _)| range.contains(&frame))
                .map_or(0.0, |(_, samples)| samples[channel]);
            assert_eq!(actual, expected, "channel {channel}, frame {frame}");
        }
    }
}

fn suspend_at(ctx: &OfflineAudioContext, time: f64) -> JsFuture {
    let suspend: js_sys::Function = js_sys::Reflect::get(ctx.as_ref(), &"suspend".into())
        .unwrap()
        .dyn_into()
        .unwrap();
    JsFuture::from(
        suspend
            .call1(ctx.as_ref(), &time.into())
            .unwrap()
            .dyn_into::<js_sys::Promise>()
            .unwrap(),
    )
}

async fn wait_for_completed(sources: &ScheduledSources, count: usize) {
    for _ in 0..100 {
        if sources
            .queued
            .iter()
            .filter(|source| source.ended.get())
            .count()
            >= count
        {
            return;
        }
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10)
                .unwrap();
        });
        JsFuture::from(promise).await.unwrap();
    }
    panic!("audio completion events were not delivered");
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_late_start_keeps_entire_pcm_until_actual_completion() {
    let ctx = context(8192);
    let suspended = suspend_at(&ctx, 0.0625);
    let rendered = JsFuture::from(ctx.start_rendering().unwrap());
    suspended.await.unwrap();
    let boundary = (ctx.current_time() * RATE as f64).round() as usize;
    assert!(boundary > BUFFER);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.0).unwrap();
    sources.prune();
    assert_eq!(sources.len(), 1);
    JsFuture::from(ctx.resume().unwrap()).await.unwrap();
    exact(
        &rendered.await.unwrap().dyn_into().unwrap(),
        &[(boundary..boundary + BUFFER, [0.5, -0.5])],
    );
    wait_for_completed(&sources, 1).await;
    sources.prune();
    assert_eq!(sources.len(), 0);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_uncanceled_buffers_overlap_control() {
    let ctx = context(8192);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.0).unwrap();
    sources.start(source(&ctx, [0.5, -0.5]), 0.125).unwrap();
    sources.start(source(&ctx, [0.25, 0.125]), 0.125).unwrap();
    exact(
        &render(&ctx).await,
        &[
            (0..BUFFER, [0.5, -0.5]),
            (6000..6000 + BUFFER, [0.75, -0.375]),
        ],
    );
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_source_limit_rejects_and_disconnects_excess_pcm() {
    let ctx = context(4096);
    let mut sources = ScheduledSources::default();
    for _ in 0..MAX_SOURCES {
        sources.start(source(&ctx, [0.0, 0.0]), 0.0).unwrap();
    }
    assert!(sources.is_full());
    assert!(sources.start(source(&ctx, [0.5, -0.5]), 0.0).is_err());
    assert_eq!(sources.len(), MAX_SOURCES);
    exact(&render(&ctx).await, &[]);
    wait_for_completed(&sources, MAX_SOURCES).await;
    sources.prune();
    assert!(!sources.is_full());
    assert_eq!(sources.len(), 0);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_cancel_removes_started_and_future_pcm_before_restart() {
    let ctx = context(8192);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.0).unwrap();
    sources.start(source(&ctx, [0.5, -0.5]), 0.125).unwrap();
    assert_eq!(sources.len(), 2);
    sources.prune();
    assert_eq!(sources.len(), 2);
    sources.cancel();
    sources.cancel();
    assert_eq!(sources.len(), 0);
    sources.start(source(&ctx, [0.25, 0.125]), 0.125).unwrap();
    exact(&render(&ctx).await, &[(6000..6000 + BUFFER, [0.25, 0.125])]);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_prunes_completed_sources_and_preserves_future_pcm() {
    let ctx = context(8192);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.0).unwrap();
    sources.start(source(&ctx, [0.25, 0.125]), 0.125).unwrap();
    let suspended = suspend_at(&ctx, 0.0625);
    let rendered = JsFuture::from(ctx.start_rendering().unwrap());
    suspended.await.unwrap();
    assert!(ctx.current_time() >= BUFFER as f64 / RATE as f64 && ctx.current_time() < 0.125);
    wait_for_completed(&sources, 1).await;
    sources.prune();
    assert_eq!(sources.len(), 1);
    JsFuture::from(ctx.resume().unwrap()).await.unwrap();
    exact(
        &rendered.await.unwrap().dyn_into().unwrap(),
        &[
            (0..BUFFER, [0.5, -0.5]),
            (6000..6000 + BUFFER, [0.25, 0.125]),
        ],
    );
    wait_for_completed(&sources, 1).await;
    sources.prune();
    assert_eq!(sources.len(), 0);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_owner_drop_cancels_unplayed_pcm() {
    let ctx = context(4096);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.0).unwrap();
    drop(sources);
    exact(&render(&ctx).await, &[]);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_failed_start_disconnects_and_is_not_retained() {
    let ctx = context(4096);
    let node = source(&ctx, [0.5, -0.5]);
    node.start().unwrap();
    let mut sources = ScheduledSources::default();
    assert!(sources.start(node, 0.0).is_err());
    assert_eq!(sources.len(), 0);
    exact(&render(&ctx).await, &[]);
}

#[wasm_bindgen_test]
async fn browser_netplay_audio_cancel_cuts_playing_pcm_at_suspension_boundary() {
    let ctx = context(8192);
    let mut sources = ScheduledSources::default();
    sources.start(source(&ctx, [0.5, -0.5]), 0.05).unwrap();
    sources.start(source(&ctx, [0.5, -0.5]), 0.125).unwrap();
    let suspended = suspend_at(&ctx, 0.0625);
    let rendered = JsFuture::from(ctx.start_rendering().unwrap());
    suspended.await.unwrap();
    let boundary = (ctx.current_time() * RATE as f64).round() as usize;
    assert!((2401..2400 + BUFFER).contains(&boundary));
    sources.cancel();
    sources.start(source(&ctx, [0.25, 0.125]), 0.125).unwrap();
    JsFuture::from(ctx.resume().unwrap()).await.unwrap();
    exact(
        &rendered.await.unwrap().dyn_into().unwrap(),
        &[
            (2400..boundary, [0.5, -0.5]),
            (6000..6000 + BUFFER, [0.25, 0.125]),
        ],
    );
}
