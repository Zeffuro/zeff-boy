use super::input::parse_zapper_event_arg;

#[test]
fn zapper_events_accept_comma_coordinates_and_semicolon_separation() {
    let events = parse_zapper_event_arg("hit@240-242:128,96;miss@300:12x34", "--zapper").unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].start_frame, 240);
    assert_eq!(events[0].end_frame, 242);
    assert_eq!((events[0].x, events[0].y), (128, 96));
    assert!(events[0].trigger);
    assert!(events[0].hit);

    assert_eq!(events[1].start_frame, 300);
    assert_eq!(events[1].end_frame, 300);
    assert_eq!((events[1].x, events[1].y), (12, 34));
    assert!(events[1].trigger);
    assert!(!events[1].hit);
}
