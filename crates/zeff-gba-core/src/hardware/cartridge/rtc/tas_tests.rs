use super::*;

#[test]
fn two_digit_year_wraps_from_2099_to_2000() {
    let mut rtc = RtcGpio {
        calendar: Calendar {
            year: 99,
            month: 12,
            day: 31,
            weekday: 4,
            hour: 23,
            minute: 59,
            second: 59,
        },
        ..RtcGpio::default()
    };

    rtc.step_cycles(CPU_CLOCK_HZ);

    assert_eq!(rtc.calendar.year, 0);
    assert_eq!(rtc.calendar.month, 1);
    assert_eq!(rtc.calendar.day, 1);
    assert_eq!(rtc.calendar.weekday, 5);
    assert_eq!(rtc.calendar.hour, 0);
    assert_eq!(rtc.calendar.minute, 0);
    assert_eq!(rtc.calendar.second, 0);
}
