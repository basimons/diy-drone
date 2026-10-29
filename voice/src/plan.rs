//! The recording checklist, read from `voice/plan.toml`: prompts, conditions, speaking styles and targets.

use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PromptKind {
    Command,
    WakeWord,
    Phrase,   // "<wake word>, <command>", derived from the other two
    NearMiss, // sounds like a command or the wake word, but isn't one
}

impl PromptKind {
    pub const ALL: [PromptKind; 4] = [
        PromptKind::Command,
        PromptKind::WakeWord,
        PromptKind::Phrase,
        PromptKind::NearMiss,
    ];

    /// The name in the manifest's `kind` column.
    pub fn as_str(self) -> &'static str {
        return match self {
            PromptKind::Command => "command",
            PromptKind::WakeWord => "wake_word",
            PromptKind::Phrase => "phrase",
            PromptKind::NearMiss => "near_miss",
        };
    }

    /// The folder under `takes/`.
    pub fn dir(self) -> &'static str {
        return match self {
            PromptKind::Command => "command",
            PromptKind::WakeWord => "wake-word",
            PromptKind::Phrase => "phrase",
            PromptKind::NearMiss => "near-miss",
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Condition {
    pub id: String,
    pub label: String,
}

/// One checklist item: say `text` in `condition_id`, `target` times.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PromptCell {
    pub kind: PromptKind,
    pub prompt_id: String,
    pub text: String,
    pub condition_id: String,
    pub target: u32,
}

/// Hands-free background sound in one condition, `target` long in total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundCell {
    pub condition_id: String,
    pub target: Duration,
}

#[derive(Debug)]
pub struct Plan {
    conditions: Vec<Condition>,
    styles: Vec<String>,
    prompt_cells: Vec<PromptCell>,
    background_cells: Vec<BackgroundCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    Toml(String),
    Io(String),
    UnknownCondition(String),
    DuplicatePromptId(String),
    BadId(String),       // a condition id that isn't a safe folder name, or a prompt that slugs to nothing
    Empty(&'static str), // names the list that is empty
    ZeroTarget(&'static str), // names the section whose target is 0
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            PlanError::Toml(e) => write!(f, "plan file is not valid: {e}"),
            PlanError::Io(e) => write!(f, "cannot read plan file: {e}"),
            PlanError::UnknownCondition(id) => write!(f, "plan uses condition \"{id}\" which is not in [[conditions]]"),
            PlanError::DuplicatePromptId(id) => write!(f, "two prompts have the same id \"{id}\""),
            PlanError::BadId(id) => write!(
                f,
                "\"{id}\" can't be used in a file name (use lowercase letters, digits and -)"
            ),
            PlanError::Empty(what) => write!(f, "plan has no {what}"),
            PlanError::ZeroTarget(section) => write!(f, "[{section}] has a target of 0"),
        };
    }
}

impl std::error::Error for PlanError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlan {
    styles: Vec<String>,
    conditions: Vec<Condition>,
    command: RawPrompts,
    wake_word: RawWakeWord,
    phrase: RawDerived,
    near_miss: RawPrompts,
    background: RawBackground,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPrompts {
    target: u32,
    conditions: Vec<String>,
    prompts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWakeWord {
    target: u32,
    conditions: Vec<String>,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDerived {
    target: u32,
    conditions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBackground {
    minutes: u32,
    conditions: Vec<String>,
}

impl Plan {
    pub fn load(path: &Path) -> Result<Plan, PlanError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| return PlanError::Io(format!("{}: {e}", path.display())))?;
        return Plan::parse(&text);
    }

    pub fn parse(text: &str) -> Result<Plan, PlanError> {
        let raw: RawPlan = toml::from_str(text).map_err(|e| return PlanError::Toml(e.to_string()))?;
        if raw.styles.is_empty() {
            return Err(PlanError::Empty("styles"));
        }
        if raw.conditions.is_empty() {
            return Err(PlanError::Empty("conditions"));
        }
        if raw.command.prompts.is_empty() {
            return Err(PlanError::Empty("command prompts"));
        }
        if raw.near_miss.prompts.is_empty() {
            return Err(PlanError::Empty("near_miss prompts"));
        }
        let targets = [
            ("command", raw.command.target),
            ("wake_word", raw.wake_word.target),
            ("phrase", raw.phrase.target),
            ("near_miss", raw.near_miss.target),
            ("background", raw.background.minutes),
        ];
        if let Some((section, _)) = targets.iter().find(|(_, target)| return *target == 0) {
            return Err(PlanError::ZeroTarget(section));
        }

        if let Some(bad) = raw
            .conditions
            .iter()
            .find(|c| return c.id.is_empty() || slug(&c.id) != c.id)
        {
            return Err(PlanError::BadId(bad.id.clone()));
        }
        let known: HashSet<&str> = raw.conditions.iter().map(|c| return c.id.as_str()).collect();
        let used = [
            &raw.command.conditions,
            &raw.wake_word.conditions,
            &raw.phrase.conditions,
            &raw.near_miss.conditions,
            &raw.background.conditions,
        ];
        if let Some(unknown) = used
            .iter()
            .flat_map(|list| return list.iter())
            .find(|id| return !known.contains(id.as_str()))
        {
            return Err(PlanError::UnknownCondition(unknown.clone()));
        }

        let phrases: Vec<String> = raw
            .command
            .prompts
            .iter()
            .map(|command| return format!("{}, {command}", raw.wake_word.prompt))
            .collect();
        let sections: [(PromptKind, &[String], &[String], u32); 4] = [
            (
                PromptKind::Command,
                &raw.command.prompts,
                &raw.command.conditions,
                raw.command.target,
            ),
            (
                PromptKind::WakeWord,
                std::slice::from_ref(&raw.wake_word.prompt),
                &raw.wake_word.conditions,
                raw.wake_word.target,
            ),
            (PromptKind::Phrase, &phrases, &raw.phrase.conditions, raw.phrase.target),
            (
                PromptKind::NearMiss,
                &raw.near_miss.prompts,
                &raw.near_miss.conditions,
                raw.near_miss.target,
            ),
        ];

        if let Some(bad) = sections
            .iter()
            .flat_map(|(_, prompts, _, _)| return prompts.iter())
            .find(|text| return slug(text).is_empty())
        {
            return Err(PlanError::BadId(bad.clone()));
        }
        let mut seen = HashSet::new();
        if let Some(duplicate) = sections
            .iter()
            .flat_map(|(_, prompts, _, _)| return prompts.iter().map(|text| return slug(text)))
            .find(|id| return !seen.insert(id.clone()))
        {
            return Err(PlanError::DuplicatePromptId(duplicate));
        }

        let prompt_cells = sections
            .iter()
            .flat_map(|(kind, prompts, conditions, target)| {
                return prompts.iter().flat_map(move |text| {
                    return conditions.iter().map(move |condition_id| {
                        return PromptCell {
                            kind: *kind,
                            prompt_id: slug(text),
                            text: text.clone(),
                            condition_id: condition_id.clone(),
                            target: *target,
                        };
                    });
                });
            })
            .collect();
        let background_cells = raw
            .background
            .conditions
            .iter()
            .map(|condition_id| {
                return BackgroundCell {
                    condition_id: condition_id.clone(),
                    target: Duration::from_secs(u64::from(raw.background.minutes) * 60),
                };
            })
            .collect();

        return Ok(Plan {
            conditions: raw.conditions,
            styles: raw.styles,
            prompt_cells,
            background_cells,
        });
    }

    pub fn conditions(&self) -> &[Condition] {
        return &self.conditions;
    }

    pub fn styles(&self) -> &[String] {
        return &self.styles;
    }

    pub fn prompt_cells(&self) -> &[PromptCell] {
        return &self.prompt_cells;
    }

    pub fn background_cells(&self) -> &[BackgroundCell] {
        return &self.background_cells;
    }
}

/// A file-system-safe id: lowercase ASCII letters and digits, with every other run of characters turned into one `-`.
pub fn slug(text: &str) -> String {
    let lowered = text.to_lowercase();
    let words: Vec<&str> = lowered
        .split(|c: char| return !c.is_ascii_alphanumeric())
        .filter(|word| return !word.is_empty())
        .collect();
    return words.join("-");
}
