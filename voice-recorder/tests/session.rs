use std::path::PathBuf;
use std::time::{Duration, Instant};

use drone_voice::{BackgroundCell, PromptCell, PromptKind, QueuedTake};
use voice_recorder::{BackgroundSession, Button, Effect, Input, TakeSession};

fn queued(prompt_id: &str, condition_id: &str) -> QueuedTake {
    return QueuedTake {
        cell: PromptCell {
            kind: PromptKind::Command,
            prompt_id: prompt_id.to_string(),
            text: prompt_id.to_string(),
            condition_id: condition_id.to_string(),
            target: 12,
        },
        style: "fast".to_string(),
    };
}

fn session(prompts: &[&str]) -> TakeSession {
    return TakeSession::new(prompts.iter().map(|p| return queued(p, "quiet-room")).collect());
}

fn current(session: &TakeSession) -> Option<&str> {
    return session.current().map(|take| return take.cell.prompt_id.as_str());
}

fn press(session: &mut TakeSession, button: Button, now: Instant) -> Vec<Effect> {
    let mut effects = session.handle(Input::Down(button), now);
    effects.extend(session.handle(Input::Up(button), now));
    return effects;
}

fn record(session: &mut TakeSession, file: &str, now: Instant) {
    assert_eq!(session.handle(Input::Down(Button::R1), now), [Effect::StartTake]);
    assert_eq!(session.handle(Input::Up(Button::R1), now), [Effect::StopTake]);
    session.saved(PathBuf::from(file));
}

#[test]
fn recording_moves_on_to_the_next_prompt() {
    let now = Instant::now();
    let mut session = session(&["flip", "land"]);
    assert_eq!((current(&session), session.position()), (Some("flip"), (1, 2)));
    record(&mut session, "a.wav", now);
    assert_eq!((current(&session), session.position()), (Some("land"), (2, 2)));
    record(&mut session, "b.wav", now);
    assert_eq!(current(&session), None);
    assert_eq!(session.summary(), [("quiet-room".to_string(), 2)]);
}

#[test]
fn redo_rejects_the_last_take_and_asks_again() {
    let now = Instant::now();
    let mut session = session(&["flip", "land"]);
    assert_eq!(press(&mut session, Button::Circle, now), []);
    record(&mut session, "a.wav", now);
    assert_eq!(
        press(&mut session, Button::Circle, now),
        [Effect::Reject(PathBuf::from("a.wav"))]
    );
    assert_eq!(current(&session), Some("flip"));
    assert_eq!(session.position(), (1, 2));
    assert_eq!(press(&mut session, Button::Circle, now), []);
    assert!(session.summary().is_empty());
}

#[test]
fn triangle_plays_the_last_take() {
    let now = Instant::now();
    let mut session = session(&["flip", "land"]);
    assert_eq!(press(&mut session, Button::Triangle, now), []);
    record(&mut session, "a.wav", now);
    assert_eq!(
        press(&mut session, Button::Triangle, now),
        [Effect::Play(PathBuf::from("a.wav"))]
    );
}

#[test]
fn skip_drops_the_cell_for_this_session_only() {
    let now = Instant::now();
    let mut session = session(&["flip", "land", "flip", "up"]);
    session.handle(Input::Down(Button::Square), now);
    assert_eq!(session.tick(now + Duration::from_millis(900)), []);
    assert_eq!(
        session.handle(Input::Up(Button::Square), now + Duration::from_millis(900)),
        []
    );
    assert_eq!(current(&session), Some("land"));
    assert_eq!(session.position(), (1, 2));
}

#[test]
fn holding_square_for_a_second_removes_the_cell() {
    let now = Instant::now();
    let mut session = session(&["flip", "land", "flip"]);
    session.handle(Input::Down(Button::Square), now);
    let removed = session.tick(now + Duration::from_secs(1));
    assert_eq!(removed, [Effect::Remove(queued("flip", "quiet-room").cell)]);
    assert_eq!(current(&session), Some("land"));
    assert_eq!(session.tick(now + Duration::from_secs(2)), []);
    assert_eq!(
        session.handle(Input::Up(Button::Square), now + Duration::from_secs(2)),
        []
    );
    assert_eq!(current(&session), Some("land"));
}

#[test]
fn skipping_only_drops_the_same_cell_not_the_same_word_elsewhere() {
    let now = Instant::now();
    let mut session = TakeSession::new(vec![queued("flip", "quiet-room"), queued("flip", "moving-sticks")]);
    press(&mut session, Button::Square, now);
    assert_eq!(
        session.current().map(|t| return t.cell.condition_id.as_str()),
        Some("moving-sticks")
    );
}

#[test]
fn paused_ignores_recording_buttons() {
    let now = Instant::now();
    let mut session = session(&["flip"]);
    press(&mut session, Button::Options, now);
    assert!(session.is_paused());
    assert_eq!(press(&mut session, Button::R1, now), []);
    assert_eq!(press(&mut session, Button::Square, now), []);
    assert_eq!(current(&session), Some("flip"));
    press(&mut session, Button::Options, now);
    assert!(!session.is_paused());
    assert_eq!(press(&mut session, Button::Cross, now), []);
}

#[test]
fn cross_while_paused_ends_the_session() {
    let now = Instant::now();
    let mut session = session(&["flip"]);
    press(&mut session, Button::Options, now);
    assert_eq!(press(&mut session, Button::Cross, now), [Effect::End]);
}

#[test]
fn other_buttons_are_ignored_while_r1_is_held() {
    let now = Instant::now();
    let mut session = session(&["flip", "land"]);
    record(&mut session, "a.wav", now);
    assert_eq!(session.handle(Input::Down(Button::R1), now), [Effect::StartTake]);
    assert_eq!(press(&mut session, Button::Circle, now), []);
    assert_eq!(press(&mut session, Button::Options, now), []);
    assert_eq!(press(&mut session, Button::Square, now), []);
    assert_eq!(session.handle(Input::Up(Button::R1), now), [Effect::StopTake]);
    assert_eq!(current(&session), Some("land"));
}

#[test]
fn a_finished_session_records_nothing() {
    let now = Instant::now();
    let mut session = session(&[]);
    assert_eq!(press(&mut session, Button::R1, now), []);
}

fn background() -> BackgroundSession {
    return BackgroundSession::new(BackgroundCell {
        condition_id: "fan".to_string(),
        target: Duration::from_secs(300),
    });
}

#[test]
fn r1_toggles_background_recording() {
    let mut session = background();
    assert_eq!(session.handle(Input::Down(Button::R1)), [Effect::StartBackground]);
    assert_eq!(session.handle(Input::Up(Button::R1)), []);
    assert!(session.is_recording());
    assert_eq!(session.handle(Input::Down(Button::R1)), [Effect::StopBackground]);
    assert!(!session.is_recording());
}

#[test]
fn a_background_session_stops_when_the_chunker_is_done() {
    let mut session = background();
    session.handle(Input::Down(Button::R1));
    session.stopped();
    assert!(!session.is_recording());
    assert_eq!(session.handle(Input::Down(Button::R1)), [Effect::StartBackground]);
}

#[test]
fn circle_rejects_only_the_last_background_chunk() {
    let mut session = background();
    session.chunk_saved(PathBuf::from("1.wav"), Duration::from_secs(60));
    session.chunk_saved(PathBuf::from("2.wav"), Duration::from_secs(20));
    assert_eq!(session.recorded(), Duration::from_secs(80));
    assert_eq!(
        session.handle(Input::Down(Button::Triangle)),
        [Effect::Play(PathBuf::from("2.wav"))]
    );
    assert_eq!(
        session.handle(Input::Down(Button::Circle)),
        [Effect::Reject(PathBuf::from("2.wav"))]
    );
    assert_eq!(session.recorded(), Duration::from_secs(60));
    assert_eq!(session.handle(Input::Down(Button::Square)), []);
}

#[test]
fn options_ends_a_background_session_only_while_stopped() {
    let mut session = background();
    session.handle(Input::Down(Button::R1));
    assert_eq!(session.handle(Input::Down(Button::Options)), []);
    assert_eq!(session.handle(Input::Down(Button::Circle)), []);
    session.handle(Input::Down(Button::R1));
    assert_eq!(session.handle(Input::Down(Button::Options)), [Effect::End]);
}
