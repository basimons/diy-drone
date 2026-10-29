use std::path::Path;
use std::time::Duration;

use drone_voice::{Plan, PlanError, PromptKind, slug};

fn shipped() -> Plan {
    return Plan::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("plan.toml")).expect("plan.toml loads");
}

const SMALL: &str = r#"
styles = ["normal"]

[[conditions]]
id = "quiet-room"
label = "Quiet room"

[command]
target = 3
conditions = ["quiet-room"]
prompts = ["flip"]

[wake_word]
target = 2
conditions = ["quiet-room"]
prompt = "Hommel"

[phrase]
target = 1
conditions = ["quiet-room"]

[near_miss]
target = 1
conditions = ["quiet-room"]
prompts = ["hummel"]

[background]
minutes = 1
conditions = ["quiet-room"]
"#;

#[test]
fn the_shipped_plan_parses_to_1382_takes() {
    let plan = shipped();
    assert_eq!(plan.prompt_cells().iter().map(|c| return c.target).sum::<u32>(), 1382);
    assert_eq!(plan.background_cells().len(), 7);
    assert!(
        plan.background_cells()
            .iter()
            .all(|c| return c.target == Duration::from_secs(300))
    );
    assert_eq!(plan.conditions().len(), 7);
    assert_eq!(plan.styles().len(), 8);
}

#[test]
fn phrases_are_derived_from_wake_word_and_commands() {
    let plan = shipped();
    let phrases: Vec<_> = plan
        .prompt_cells()
        .iter()
        .filter(|c| return c.kind == PromptKind::Phrase)
        .collect();
    assert_eq!(phrases.len(), 19 * 2);
    assert!(phrases.iter().any(|c| {
        return c.text == "Hommel, turn left" && c.prompt_id == "hommel-turn-left" && c.condition_id == "quiet-room";
    }));
}

#[test]
fn a_small_plan_has_its_cells_in_plan_order() {
    let plan = Plan::parse(SMALL).unwrap();
    let ids: Vec<_> = plan
        .prompt_cells()
        .iter()
        .map(|c| return (c.kind, c.prompt_id.as_str(), c.target))
        .collect();
    assert_eq!(
        ids,
        [
            (PromptKind::Command, "flip", 3),
            (PromptKind::WakeWord, "hommel", 2),
            (PromptKind::Phrase, "hommel-flip", 1),
            (PromptKind::NearMiss, "hummel", 1),
        ]
    );
    assert_eq!(plan.background_cells()[0].condition_id, "quiet-room");
    assert_eq!(plan.background_cells()[0].target, Duration::from_secs(60));
}

#[test]
fn slug_makes_file_safe_names() {
    assert_eq!(slug("turn left"), "turn-left");
    assert_eq!(slug("Hommel, flip"), "hommel-flip");
    assert_eq!(slug("flat / tired"), "flat-tired");
    assert_eq!(slug("Bram S."), "bram-s");
    assert_eq!(slug("  Rommel!! "), "rommel");
}

#[test]
fn rejects_an_unknown_condition() {
    let text = SMALL.replace(
        "conditions = [\"quiet-room\"]\nprompts = [\"flip\"]",
        "conditions = [\"attic\"]\nprompts = [\"flip\"]",
    );
    assert_eq!(
        Plan::parse(&text).unwrap_err(),
        PlanError::UnknownCondition("attic".to_string())
    );
}

#[test]
fn rejects_duplicate_prompt_ids() {
    let text = SMALL.replace("prompts = [\"hummel\"]", "prompts = [\"Flip!\"]");
    assert_eq!(
        Plan::parse(&text).unwrap_err(),
        PlanError::DuplicatePromptId("flip".to_string())
    );
}

#[test]
fn rejects_empty_styles() {
    let text = SMALL.replace("styles = [\"normal\"]", "styles = []");
    assert!(matches!(Plan::parse(&text).unwrap_err(), PlanError::Empty(_)));
}

#[test]
fn rejects_a_zero_target() {
    let text = SMALL.replace("target = 3", "target = 0");
    assert!(matches!(Plan::parse(&text).unwrap_err(), PlanError::ZeroTarget(_)));
}

#[test]
fn rejects_bad_toml() {
    assert!(matches!(Plan::parse("styles = [").unwrap_err(), PlanError::Toml(_)));
}

#[test]
fn rejects_condition_ids_that_are_not_safe_folder_names() {
    for bad in ["outdoors/windy", "..", "", "Quiet Room"] {
        let text = SMALL
            .replace("id = \"quiet-room\"", &format!("id = \"{bad}\""))
            .replace("\"quiet-room\"", &format!("\"{bad}\""));
        assert_eq!(
            Plan::parse(&text).unwrap_err(),
            PlanError::BadId(bad.to_string()),
            "{bad}"
        );
    }
}

#[test]
fn rejects_prompts_without_letters_or_digits() {
    let text = SMALL.replace("prompts = [\"hummel\"]", "prompts = [\"?!\"]");
    assert_eq!(Plan::parse(&text).unwrap_err(), PlanError::BadId("?!".to_string()));
}
