use core::time::Duration;
use drone_link::time::SessionTime;

#[test]
fn since_is_the_time_in_between() {
    assert_eq!(SessionTime(1_500).since(SessionTime(500)), Duration::from_micros(1_000));
}

#[test]
fn since_works_across_the_wrap() {
    // The session clock is a u32 of µs and wraps after ~71 minutes (protocol.md §5.1).
    assert_eq!(
        SessionTime(100).since(SessionTime(u32::MAX - 99)),
        Duration::from_micros(200)
    );
}
