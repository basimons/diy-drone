use std::path::Path;

use drone_voice::{Plan, PromptKind, Selection};
use voice_recorder::{Button, Row, Setup, Start};

fn plan() -> Plan {
    return Plan::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../voice/plan.toml")).unwrap();
}

fn move_to(setup: &mut Setup, row: Row) {
    let rows = setup.rows();
    let index = |r: Row| return rows.iter().position(|&x| return x == r).unwrap();
    while setup.cursor() != row {
        let direction = if index(setup.cursor()) < index(row) {
            Button::Down
        } else {
            Button::Up
        };
        setup.handle(direction);
    }
}

fn tick(setup: &mut Setup, row: Row) -> Option<Start> {
    move_to(setup, row);
    return setup.handle(Button::Cross);
}

#[test]
fn starts_with_every_prompt_kind_and_no_condition_ticked() {
    let setup = Setup::new(&plan());
    assert!(
        PromptKind::ALL
            .iter()
            .all(|&kind| return setup.is_ticked(Row::Kind(kind)))
    );
    assert!((0..7).all(|i| return !setup.is_ticked(Row::Condition(i))));
    assert!(!setup.is_ticked(Row::Background));
    assert_eq!(setup.length(), 120);
    assert_eq!(setup.cursor(), Row::Condition(0));
    assert_eq!(setup.start(), None);
}

#[test]
fn rows_list_conditions_then_kinds_then_background_length_and_start() {
    let rows = Setup::new(&plan()).rows();
    assert_eq!(rows.len(), 7 + 4 + 3);
    assert_eq!(rows[0], Row::Condition(0));
    assert_eq!(rows[7], Row::Kind(PromptKind::Command));
    assert_eq!(&rows[11..], [Row::Background, Row::Length, Row::Start]);
}

#[test]
fn the_cursor_stays_on_the_rows() {
    let mut setup = Setup::new(&plan());
    setup.handle(Button::Up);
    assert_eq!(setup.cursor(), Row::Condition(0));
    (0..50).for_each(|_| return drop(setup.handle(Button::Down)));
    assert_eq!(setup.cursor(), Row::Start);
}

#[test]
fn ticking_conditions_gives_a_take_session() {
    let mut setup = Setup::new(&plan());
    tick(&mut setup, Row::Condition(0));
    tick(&mut setup, Row::Condition(3));
    tick(&mut setup, Row::Kind(PromptKind::Phrase));
    let expected = Selection {
        kinds: vec![PromptKind::Command, PromptKind::WakeWord, PromptKind::NearMiss],
        conditions: vec!["quiet-room".to_string(), "moving-sticks".to_string()],
        length: 120,
    };
    assert_eq!(setup.start(), Some(Start::Takes(expected.clone())));
    assert_eq!(tick(&mut setup, Row::Start), Some(Start::Takes(expected)));
}

#[test]
fn cross_on_start_does_nothing_when_nothing_can_start() {
    let mut setup = Setup::new(&plan());
    assert_eq!(tick(&mut setup, Row::Start), None);
}

#[test]
fn no_prompt_kind_means_no_session() {
    let mut setup = Setup::new(&plan());
    tick(&mut setup, Row::Condition(0));
    PromptKind::ALL
        .iter()
        .for_each(|&kind| return drop(tick(&mut setup, Row::Kind(kind))));
    assert_eq!(setup.start(), None);
}

#[test]
fn background_is_its_own_session_type() {
    let mut setup = Setup::new(&plan());
    tick(&mut setup, Row::Background);
    assert!(
        PromptKind::ALL
            .iter()
            .all(|&kind| return !setup.is_ticked(Row::Kind(kind)))
    );
    assert_eq!(setup.start(), None);
    tick(&mut setup, Row::Condition(1));
    let Some(Start::Background(cell)) = setup.start() else {
        panic!("expected a background session");
    };
    assert_eq!(cell.condition_id, "tv-music");
    tick(&mut setup, Row::Condition(2));
    assert_eq!(setup.start(), None);
    tick(&mut setup, Row::Kind(PromptKind::Command));
    assert!(!setup.is_ticked(Row::Background));
    assert!(matches!(setup.start(), Some(Start::Takes(_))));
}

#[test]
fn left_and_right_change_the_length_by_10_within_10_to_500() {
    let mut setup = Setup::new(&plan());
    move_to(&mut setup, Row::Length);
    setup.handle(Button::Right);
    assert_eq!(setup.length(), 130);
    (0..100).for_each(|_| return drop(setup.handle(Button::Right)));
    assert_eq!(setup.length(), 500);
    (0..100).for_each(|_| return drop(setup.handle(Button::Left)));
    assert_eq!(setup.length(), 10);
    move_to(&mut setup, Row::Start);
    setup.handle(Button::Left);
    assert_eq!(setup.length(), 10);
}
