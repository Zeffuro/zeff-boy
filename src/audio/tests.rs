use super::*;

#[test]
fn ring_buffer_capacity_44100() {
    assert_eq!(ring_buffer_capacity(44100), 17640);
}

#[test]
fn ring_buffer_capacity_48000() {
    assert_eq!(ring_buffer_capacity(48000), 19200);
}

fn push_samples(producer: &mut rtrb::Producer<f32>, samples: &[f32]) {
    if let Ok(mut chunk) = producer.write_chunk_uninit(samples.len()) {
        let (first, second) = chunk.as_mut_slices();
        for (dst, &src) in first.iter_mut().zip(samples.iter()) {
            dst.write(src);
        }
        for (dst, &src) in second.iter_mut().zip(samples[first.len()..].iter()) {
            dst.write(src);
        }
        unsafe { chunk.commit_all(); }
    }
}

#[test]
fn fill_mono_exact() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.5, -0.5, 0.25]);

    let mut data = vec![0.0f32; 3];
    fill_output_f32(&mut data, 1, &mut consumer);
    assert_eq!(data, vec![0.5, -0.5, 0.25]);
}

#[test]
fn fill_mono_underflow_pads_silence() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2]);

    let mut data = vec![9.9f32; 5];
    fill_output_f32(&mut data, 1, &mut consumer);
    assert_eq!(data, vec![0.1, 0.2, 0.0, 0.0, 0.0]);
}

#[test]
fn fill_mono_empty_buffer_is_silence() {
    let (_producer, mut consumer) = rtrb::RingBuffer::<f32>::new(64);

    let mut data = vec![1.0f32; 4];
    fill_output_f32(&mut data, 1, &mut consumer);
    assert_eq!(data, vec![0.0; 4]);
}

#[test]
fn fill_stereo_maps_lr_pairs() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2, 0.3, 0.4]);

    let mut data = vec![0.0f32; 4]; // 2 frames * 2 channels
    fill_output_f32(&mut data, 2, &mut consumer);
    assert_eq!(data, vec![0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn fill_stereo_underflow_pads_silence() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.5, 0.6]);

    let mut data = vec![9.0f32; 6];
    fill_output_f32(&mut data, 2, &mut consumer);
    assert_eq!(data, vec![0.5, 0.6, 0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn fill_multichannel_mixes_to_surround() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.4, 0.6]);

    let mut data = vec![0.0f32; 4];
    fill_output_f32(&mut data, 4, &mut consumer);
    assert_eq!(data[0], 0.4);
    assert_eq!(data[1], 0.6);
    assert_eq!(data[2], 0.5);
    assert_eq!(data[3], 0.5);
}

#[test]
fn fill_stereo_empty_is_silence() {
    let (_producer, mut consumer) = rtrb::RingBuffer::<f32>::new(64);
    let mut data = vec![1.0f32; 4];
    fill_output_f32(&mut data, 2, &mut consumer);
    assert_eq!(data, vec![0.0; 4]);
}

#[test]
fn fill_stereo_odd_samples_drops_trailing() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(64);
    push_samples(&mut producer, &[0.1, 0.2, 0.3]);

    let mut data = vec![0.0f32; 4];
    fill_output_f32(&mut data, 2, &mut consumer);

    assert_eq!(data[0], 0.1);
    assert_eq!(data[1], 0.2);
    assert_eq!(data[2], 0.0);
    assert_eq!(data[3], 0.0);
}

