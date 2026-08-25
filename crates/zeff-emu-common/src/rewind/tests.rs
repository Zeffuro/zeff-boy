use super::*;

#[test]
fn push_and_pop_round_trips_data() {
    let mut buf = RewindBuffer::new(10, 4);
    let state = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let fb = vec![10u8, 20, 30, 40];
    buf.push(&state, &fb);
    assert_eq!(buf.len(), 1);
    let frame = buf.pop().unwrap();
    assert_eq!(frame.state_bytes, state);
    assert_eq!(frame.framebuffer, fb);
    assert!(buf.is_empty());
}

#[test]
fn pop_empty_returns_none() {
    let mut buf = RewindBuffer::new(10, 4);
    assert!(buf.pop().is_none());
}

#[test]
fn capacity_limits_snapshots() {
    let mut buf = RewindBuffer::new(2, 4);
    let cap = buf.capacity();
    for i in 0..(cap + 10) {
        buf.push(&[i as u8], &[]);
    }
    assert_eq!(buf.len(), cap);
}

#[test]
fn capacity_tracks_the_system_frame_duration() {
    let sixty_hz = RewindBuffer::new_with_frame_duration(10, 4, 16_666_667);
    let wonder_swan = RewindBuffer::new_with_frame_duration(10, 4, 13_250_298);

    assert_eq!(sixty_hz.capacity(), 150);
    assert_eq!(wonder_swan.capacity(), 189);
}

#[test]
fn fill_ratio_tracks_usage() {
    let mut buf = RewindBuffer::new(10, 4);
    assert_eq!(buf.fill_ratio(), 0.0);
    let cap = buf.capacity();
    for i in 0..cap {
        buf.push(&[i as u8], &[]);
    }
    assert!((buf.fill_ratio() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn clear_resets_buffer() {
    let mut buf = RewindBuffer::new(10, 4);
    buf.push(&[42], &[1]);
    buf.push(&[43], &[2]);
    assert_eq!(buf.len(), 2);
    buf.clear();
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
}

#[test]
fn tick_fires_at_interval() {
    let mut buf = RewindBuffer::new(10, 4);
    assert!(!buf.advance_frames(1));
    assert!(!buf.advance_frames(1));
    assert!(!buf.advance_frames(1));
    assert!(buf.advance_frames(1));
    assert!(!buf.advance_frames(1));
}

#[test]
fn capture_cadence_uses_emulated_frames_in_batched_steps() {
    let mut buf = RewindBuffer::new(10, 4);
    assert!(!buf.advance_frames(2));
    assert!(buf.advance_frames(3));
    buf.push(&[5], &[]);
    assert!(!buf.advance_frames(2));
    assert!(buf.advance_frames(2));
    buf.push(&[9], &[]);

    let latest = buf.pop().unwrap();
    assert_eq!(latest.state_bytes, [9]);
    assert_eq!(latest.rewound_frames, 0);
    let earlier = buf.pop().unwrap();
    assert_eq!(earlier.state_bytes, [5]);
    assert_eq!(earlier.rewound_frames, 4);
}

#[test]
fn pop_reports_actual_emulated_time_across_uneven_batches() {
    let mut buf = RewindBuffer::new(10, 4);
    assert!(buf.advance_frames(5));
    buf.push(&[5], &[]);
    assert!(buf.advance_frames(7));
    buf.push(&[12], &[]);
    assert!(!buf.advance_frames(2));

    let frame = buf.pop_steps(2).unwrap();
    assert_eq!(frame.state_bytes, [5]);
    assert_eq!(frame.rewound_frames, 9);
}

#[test]
fn pop_returns_most_recent_first() {
    let mut buf = RewindBuffer::new(10, 4);
    buf.push(&[1], &[10]);
    buf.push(&[2], &[20]);
    buf.push(&[3], &[30]);
    let f3 = buf.pop().unwrap();
    assert_eq!(f3.state_bytes, vec![3]);
    assert_eq!(f3.framebuffer, vec![30]);
    let f2 = buf.pop().unwrap();
    assert_eq!(f2.state_bytes, vec![2]);
    assert_eq!(f2.framebuffer, vec![20]);
    let f1 = buf.pop().unwrap();
    assert_eq!(f1.state_bytes, vec![1]);
    assert_eq!(f1.framebuffer, vec![10]);
}

#[test]
fn pop_steps_skips_newer_snapshots() {
    let mut buf = RewindBuffer::new(10, 1);
    for value in 1..=5 {
        buf.push(&[value], &[value + 10]);
    }

    let frame = buf.pop_steps(3).unwrap();
    assert_eq!(frame.state_bytes, vec![3]);
    assert_eq!(frame.framebuffer, vec![13]);
    assert_eq!(buf.len(), 2);
}

#[test]
fn framebuffer_stored_and_recovered() {
    let mut buf = RewindBuffer::new(10, 4);
    let state = vec![0xAA; 100];
    let fb = vec![0xBB; 160 * 144 * 4];
    buf.push(&state, &fb);
    let frame = buf.pop().unwrap();
    assert_eq!(frame.state_bytes, state);
    assert_eq!(frame.framebuffer, fb);
}

#[test]
fn peek_returns_most_recent_without_removing() {
    let mut buf = RewindBuffer::new(10, 4);
    buf.push(&[1], &[10]);
    buf.push(&[2], &[20]);
    let peeked = buf.peek().unwrap();
    assert_eq!(peeked.state_bytes, vec![2]);
    assert_eq!(peeked.framebuffer, vec![20]);
    assert_eq!(buf.len(), 2);
    let popped = buf.pop().unwrap();
    assert_eq!(popped.state_bytes, vec![2]);
    assert_eq!(buf.len(), 1);
}

#[test]
fn peek_empty_returns_none() {
    let buf = RewindBuffer::new(10, 4);
    assert!(buf.peek().is_none());
}
