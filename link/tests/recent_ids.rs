use drone_link::recent_ids::{RecentIds, Seen};

#[test]
fn an_id_is_new_once_then_a_repeat() {
    let mut recent = RecentIds::<u16, 4>::new();
    assert_eq!(recent.remember(7), Seen::New);
    assert_eq!(recent.remember(7), Seen::Repeat);
    assert_eq!(recent.remember(8), Seen::New);
}

#[test]
fn only_the_last_n_are_remembered() {
    let mut recent = RecentIds::<u16, 3>::new();
    for id in [1, 2, 3, 4] {
        recent.remember(id);
    }
    assert!(!recent.contains(1));
    assert!(recent.contains(2) && recent.contains(3) && recent.contains(4));
    assert_eq!(recent.remember(1), Seen::New);
    assert!(!recent.contains(2));
}

#[test]
fn a_repeat_does_not_make_an_id_newer() {
    // "The last N handled": a repeat isn't handled again, so it doesn't push out another ID.
    let mut recent = RecentIds::<u16, 2>::new();
    recent.remember(1);
    recent.remember(2);
    recent.remember(1);
    recent.remember(3);
    assert!(!recent.contains(1));
    assert!(recent.contains(2) && recent.contains(3));
}

#[test]
fn works_with_any_id_type() {
    let mut recent = RecentIds::<drone_common::wire::message::RequestId, 16>::new();
    assert_eq!(
        recent.remember(drone_common::wire::message::RequestId(65535)),
        Seen::New
    );
}

#[test]
fn every_value_counts_even_zero() {
    // Request IDs and Event IDs wrap and use every value (protocol.md §5.0), so an empty slot isn't a zero.
    let mut recent = RecentIds::<u16, 4>::new();
    assert!(!recent.contains(0));
    assert_eq!(recent.remember(0), Seen::New);
}
