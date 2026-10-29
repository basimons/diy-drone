use drone_voice::{BackgroundCell, Plan, PromptKind, Selection};

use crate::Button;

const DEFAULT_LENGTH: usize = 120;
const LENGTH_STEP: usize = 10;
const LONGEST: usize = 500;

/// A line on the start-up screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Condition(usize), // index into the plan's conditions
    Kind(PromptKind),
    Background,
    Length,
    Start,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    Takes(Selection),
    Background(BackgroundCell),
}

/// The start-up screen: where you are right now and what to record.
#[derive(Debug)]
pub struct Setup {
    conditions: Vec<(String, bool)>,
    kinds: Vec<(PromptKind, bool)>,
    background: bool,
    background_cells: Vec<BackgroundCell>,
    length: usize,
    cursor: usize,
}

impl Setup {
    pub fn new(plan: &Plan) -> Setup {
        return Setup {
            conditions: plan.conditions().iter().map(|c| return (c.id.clone(), false)).collect(),
            kinds: PromptKind::ALL.iter().map(|&kind| return (kind, true)).collect(),
            background: false,
            background_cells: plan.background_cells().to_vec(),
            length: DEFAULT_LENGTH,
            cursor: 0,
        };
    }

    pub fn rows(&self) -> Vec<Row> {
        let conditions = (0..self.conditions.len()).map(Row::Condition);
        let kinds = self.kinds.iter().map(|&(kind, _)| return Row::Kind(kind));
        return conditions
            .chain(kinds)
            .chain([Row::Background, Row::Length, Row::Start])
            .collect();
    }

    pub fn cursor(&self) -> Row {
        return self.rows()[self.cursor];
    }

    pub fn length(&self) -> usize {
        return self.length;
    }

    pub fn is_ticked(&self, row: Row) -> bool {
        return match row {
            Row::Condition(i) => self.conditions[i].1,
            Row::Kind(kind) => self.kinds.iter().any(|&(k, ticked)| return k == kind && ticked),
            Row::Background => self.background,
            Row::Length | Row::Start => false,
        };
    }

    /// D-pad moves and changes the length; ✕ ticks the row under the cursor, or starts on the Start row.
    pub fn handle(&mut self, button: Button) -> Option<Start> {
        let last = self.rows().len() - 1;
        match (button, self.cursor()) {
            (Button::Up, _) => self.cursor = self.cursor.saturating_sub(1),
            (Button::Down, _) => self.cursor = (self.cursor + 1).min(last),
            (Button::Left, Row::Length) => self.length = self.length.saturating_sub(LENGTH_STEP).max(LENGTH_STEP),
            (Button::Right, Row::Length) => self.length = (self.length + LENGTH_STEP).min(LONGEST),
            (Button::Cross, Row::Start) => return self.start(),
            (Button::Cross, row) => self.toggle(row),
            _ => {}
        }
        return None;
    }

    fn toggle(&mut self, row: Row) {
        match row {
            Row::Condition(i) => self.conditions[i].1 = !self.conditions[i].1,
            Row::Kind(kind) => {
                self.kinds
                    .iter_mut()
                    .filter(|(k, _)| return *k == kind)
                    .for_each(|(_, ticked)| *ticked = !*ticked);
                self.background = false;
            }
            Row::Background => {
                self.background = !self.background;
                self.kinds.iter_mut().for_each(|(_, ticked)| *ticked = false);
            }
            Row::Length | Row::Start => {}
        }
    }

    /// What ✕ on Start would begin, or `None` while the ticks don't make a session.
    pub fn start(&self) -> Option<Start> {
        let conditions: Vec<String> = self
            .conditions
            .iter()
            .filter(|(_, ticked)| return *ticked)
            .map(|(id, _)| return id.clone())
            .collect();
        if self.background {
            let [only] = conditions.as_slice() else {
                return None;
            };
            return self
                .background_cells
                .iter()
                .find(|cell| return cell.condition_id == *only)
                .cloned()
                .map(Start::Background);
        }
        let kinds: Vec<PromptKind> = self
            .kinds
            .iter()
            .filter(|(_, ticked)| return *ticked)
            .map(|&(kind, _)| return kind)
            .collect();
        if kinds.is_empty() || conditions.is_empty() {
            return None;
        }
        return Some(Start::Takes(Selection {
            kinds,
            conditions,
            length: self.length,
        }));
    }
}
