use chrono::Utc;
use drone_voice::{Clip, MicInfo, NewTake, Plan, PromptCell, PromptKind, QueuedTake, Selection, Store, pick};
use rand::SeedableRng;
use rand::rngs::StdRng;

fn plan(command_target: u32, commands: &[&str]) -> Plan {
    let prompts = commands
        .iter()
        .map(|c| return format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    return Plan::parse(&format!(
        r#"
styles = ["normal", "fast", "slow"]
[[conditions]]
id = "quiet-room"
label = "Quiet room"
[[conditions]]
id = "moving-sticks"
label = "While moving the sticks"
[command]
target = {command_target}
conditions = ["quiet-room", "moving-sticks"]
prompts = [{prompts}]
[wake_word]
target = 4
conditions = ["quiet-room", "moving-sticks"]
prompt = "Hommel"
[phrase]
target = 1
conditions = ["quiet-room"]
[near_miss]
target = 2
conditions = ["moving-sticks"]
prompts = ["hummel"]
[background]
minutes = 1
conditions = ["quiet-room"]
"#
    ))
    .unwrap();
}

fn commands_in_quiet_room(length: usize) -> Selection {
    return Selection {
        kinds: vec![PromptKind::Command],
        conditions: vec!["quiet-room".to_string()],
        length,
    };
}

fn run(plan: &Plan, store: &Store, selection: &Selection, seed: u64) -> Vec<QueuedTake> {
    return pick(
        plan,
        &store.progress(),
        |cell| return store.is_removed(cell),
        selection,
        &mut StdRng::seed_from_u64(seed),
    );
}

fn cell<'a>(plan: &'a Plan, prompt_id: &str, condition_id: &str) -> &'a PromptCell {
    return plan
        .prompt_cells()
        .iter()
        .find(|c| return c.prompt_id == prompt_id && c.condition_id == condition_id)
        .unwrap();
}

fn save(store: &mut Store, cell: &PromptCell, count: usize) {
    let mic = MicInfo {
        device_name: "DualSense".to_string(),
        sample_rate: 48_000,
        channels: 1,
    };
    let samples = [1_i16; 10];
    for i in 0..count {
        let time = Utc::now() + chrono::Duration::milliseconds(i as i64);
        let take = NewTake {
            clip: Clip::Prompt(cell),
            samples: &samples,
            style: Some("normal"),
            speaker: "s",
            session_id: "s",
            time,
            mic: &mic,
            capture_volume: None,
            warnings: &[],
        };
        store.save(take).unwrap();
    }
}

#[test]
fn only_selected_kinds_and_conditions_come_up() {
    let dir = tempfile::tempdir().unwrap();
    let (plan, store) = (plan(5, &["flip", "land"]), Store::open(dir.path()).unwrap());
    let picked = run(&plan, &store, &commands_in_quiet_room(100), 1);
    assert_eq!(picked.len(), 10);
    assert!(
        picked
            .iter()
            .all(|t| return t.cell.kind == PromptKind::Command && t.cell.condition_id == "quiet-room")
    );
}

#[test]
fn several_kinds_and_conditions_can_be_selected() {
    let dir = tempfile::tempdir().unwrap();
    let (plan, store) = (plan(1, &["flip"]), Store::open(dir.path()).unwrap());
    let selection = Selection {
        kinds: vec![PromptKind::Command, PromptKind::NearMiss],
        conditions: vec!["quiet-room".to_string(), "moving-sticks".to_string()],
        length: 100,
    };
    // flip × 2 conditions + hummel × 2 takes in moving-sticks
    assert_eq!(run(&plan, &store, &selection, 1).len(), 4);
}

#[test]
fn removed_and_complete_cells_never_come_up() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan(3, &["flip", "land", "up"]);
    let mut store = Store::open(dir.path()).unwrap();
    save(&mut store, cell(&plan, "flip", "quiet-room"), 3);
    store.remove(cell(&plan, "land", "quiet-room"), Utc::now()).unwrap();
    let picked = run(&plan, &store, &commands_in_quiet_room(100), 7);
    assert_eq!(picked.len(), 3);
    assert!(picked.iter().all(|t| return t.cell.prompt_id == "up"));
}

#[test]
fn never_asks_more_than_a_cell_still_needs() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan(2, &["flip", "land", "up"]);
    let mut store = Store::open(dir.path()).unwrap();
    save(&mut store, cell(&plan, "land", "quiet-room"), 1);
    let picked = run(&plan, &store, &commands_in_quiet_room(50), 3);
    let count = |id: &str| return picked.iter().filter(|t| return t.cell.prompt_id == id).count();
    assert_eq!((count("flip"), count("land"), count("up")), (2, 1, 2));
}

#[test]
fn the_same_prompt_never_comes_twice_in_a_row_when_there_is_a_choice() {
    let dir = tempfile::tempdir().unwrap();
    let (plan, store) = (plan(10, &["flip", "land"]), Store::open(dir.path()).unwrap());
    for seed in 0..100 {
        let picked = run(&plan, &store, &commands_in_quiet_room(20), seed);
        assert_eq!(picked.len(), 20);
        assert!(
            picked
                .windows(2)
                .all(|pair| return pair[0].cell.prompt_id != pair[1].cell.prompt_id),
            "seed {seed}"
        );
    }
}

#[test]
fn one_prompt_left_still_comes_up_back_to_back() {
    let dir = tempfile::tempdir().unwrap();
    let (plan, store) = (plan(4, &["flip"]), Store::open(dir.path()).unwrap());
    assert_eq!(run(&plan, &store, &commands_in_quiet_room(10), 1).len(), 4);
}

#[test]
fn stops_early_when_everything_is_done() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan(5, &["flip"]);
    let mut store = Store::open(dir.path()).unwrap();
    save(&mut store, cell(&plan, "flip", "quiet-room"), 5);
    assert!(run(&plan, &store, &commands_in_quiet_room(120), 1).is_empty());
}

#[test]
fn emptier_cells_come_up_more() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan(20, &["flip", "land", "up"]);
    let mut store = Store::open(dir.path()).unwrap();
    save(&mut store, cell(&plan, "flip", "quiet-room"), 18);
    let (mut flips, mut lands) = (0, 0);
    for seed in 0..200 {
        let picked = run(&plan, &store, &commands_in_quiet_room(5), seed);
        flips += picked.iter().filter(|t| return t.cell.prompt_id == "flip").count();
        lands += picked.iter().filter(|t| return t.cell.prompt_id == "land").count();
    }
    assert!(lands > flips * 3, "land {lands} vs flip {flips}");
}

#[test]
fn every_take_gets_a_style_from_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let (plan, store) = (plan(10, &["flip", "land"]), Store::open(dir.path()).unwrap());
    let picked = run(&plan, &store, &commands_in_quiet_room(20), 5);
    assert!(picked.iter().all(|t| return plan.styles().contains(&t.style)));
    assert!(picked.iter().any(|t| return t.style != picked[0].style));
}
