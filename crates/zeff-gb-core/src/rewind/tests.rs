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
    assert!(!buf.tick()); // 1
    assert!(!buf.tick()); // 2
    assert!(!buf.tick()); // 3
    assert!(buf.tick()); // 4 -> fires
    assert!(!buf.tick()); // 1 again
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

