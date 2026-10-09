use super::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{AudioBuffer, AudioContext, OfflineAudioContext};

struct OfflineOutput {
    audio: AudioOutput,
    original: AudioContext,
}

impl OfflineOutput {
    fn new(ctx: &OfflineAudioContext) -> Self {
        let mut audio = AudioOutput::new(Some(48_000)).unwrap();
        // The scheduler uses inherited BaseAudioContext APIs on the real offline graph.
        let original = std::mem::replace(&mut audio.ctx, ctx.clone().unchecked_into());
        audio.resume_requested = true;
        Self { audio, original }
    }
}

impl Drop for OfflineOutput {
    fn drop(&mut self) {
        self.audio.ctx = self.original.clone();
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

#[wasm_bindgen_test]
async fn browser_netplay_audio_horizon_backlog_plays_without_new_frame_audio() {
    let frames = BUFFER_FRAMES * 14;
    let input: Vec<_> = (0..frames * 2)
        .flat_map(|frame| {
            [
                (frame % 256 + 1) as f32 / 512.0,
                -((frame / 256 % 32 + 1) as f32) / 64.0,
            ]
        })
        .collect();
    let expected: Vec<_> = input
        .as_chunks::<2>()
        .0
        .iter()
        .step_by(2)
        .flat_map(|frame| [frame[0] * 0.5, frame[1] * 0.5])
        .collect();
    for pump in [false, true] {
        let ctx = OfflineAudioContext::new_with_number_of_channels_and_length_and_sample_rate(
            2,
            (frames + BUFFER_FRAMES) as u32,
            48_000.0,
        )
        .unwrap();
        let mut output = OfflineOutput::new(&ctx);
        output.audio.queue_samples(
            &input,
            &AudioQueueConfig {
                master_volume: 0.5,
                playback_speed: 2,
                mute_during_fast_forward: false,
                low_pass_enabled: false,
                low_pass_cutoff_hz: 20_000,
            },
        );
        assert_eq!(output.audio.scheduled_sources, 6);
        assert_eq!(output.audio.buffer, expected[BUFFER_FRAMES * 6 * 2..]);
        let first = suspend_at(&ctx, 0.0625);
        let second = suspend_at(&ctx, 0.17);
        let rendered = JsFuture::from(ctx.start_rendering().unwrap());
        first.await.unwrap();
        assert!((0.0625..0.07).contains(&ctx.current_time()));
        if pump {
            output.audio.pump_queued_samples();
        }
        JsFuture::from(ctx.resume().unwrap()).await.unwrap();
        second.await.unwrap();
        assert!((0.17..0.18).contains(&ctx.current_time()));
        if pump {
            output.audio.pump_queued_samples();
        }
        if pump {
            assert!(output.audio.buffer.is_empty());
            assert_eq!(output.audio.scheduled_sources, 14);
        } else {
            assert_eq!(output.audio.buffer, expected[BUFFER_FRAMES * 6 * 2..]);
            assert_eq!(output.audio.scheduled_sources, 6);
        }
        JsFuture::from(ctx.resume().unwrap()).await.unwrap();
        let rendered: AudioBuffer = rendered.await.unwrap().dyn_into().unwrap();
        let played = if pump { frames } else { BUFFER_FRAMES * 6 };
        for channel in 0..2 {
            for (frame, actual) in rendered
                .get_channel_data(channel)
                .unwrap()
                .iter()
                .enumerate()
            {
                let expected = if frame < played {
                    expected[frame * 2 + channel as usize]
                } else {
                    0.0
                };
                assert_eq!(
                    *actual, expected,
                    "pump {pump}, channel {channel}, frame {frame}"
                );
            }
        }
        output.audio.discard_queued_samples();
        output.audio.pump_queued_samples();
        assert!(output.audio.buffer.is_empty());
        assert_eq!(output.audio.sources.len(), 0);
    }
}
