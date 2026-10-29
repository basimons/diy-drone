//! The window: runs the tested state machines against the real mic, controller, store and speakers.

use std::time::{Duration, Instant};

use chrono::Utc;
use drone_voice::{
    BackgroundCell, Chunker, Clip, Mic, MicEvent, NewTake, Plan, PromptKind, PushToTalk, Segment, Store, Warning,
    capture_volume, pick, read_wav, warnings,
};
use eframe::egui;
use voice_recorder::{BackgroundSession, Button, Effect, Input, Row, Setup, Start, TakeSession};

use crate::input::{Controller, keyboard};
use crate::playback::{Playback, play};

const MIC_RETRY: Duration = Duration::from_secs(1);
const WAVEFORM_COLUMNS: usize = 300;
const TIME_IN_SESSION_ID: &str = "%Y-%m-%dT%H-%M-%SZ";

pub struct App {
    plan: Plan,
    store: Store,
    mic: Result<Mic, String>,
    mic_tried: Instant,
    controller: Controller,
    speaker: String,
    screen: Screen,
    level: f32, // peak of the latest audio block, 0..1
    last: Option<LastTake>,
    message: Option<String>, // the latest problem, shown until the next one
    playback: Option<Playback>,
}

enum Screen {
    Setup(Setup),
    Takes(Takes),
    Background(Background),
    Summary(Vec<String>),
}

struct Takes {
    session: TakeSession,
    ptt: PushToTalk,
    rate: u32,
    recording: bool,
    id: String,
}

struct Background {
    session: BackgroundSession,
    chunker: Option<Chunker>, // Some while recording
    id: String,
}

struct LastTake {
    waveform: Vec<(f32, f32)>, // (min, max) per column, -1..1
    warnings: Vec<Warning>,
}

impl App {
    pub fn new(plan: Plan, store: Store) -> App {
        let speaker = store.last_speaker().unwrap_or("speaker").to_string();
        return App {
            screen: Screen::Setup(Setup::new(&plan)),
            plan,
            store,
            mic: Mic::open().map_err(|e| return e.to_string()),
            mic_tried: Instant::now(),
            controller: Controller::new(),
            speaker,
            level: 0.0,
            last: None,
            message: None,
            playback: None,
        };
    }

    fn reconnect_mic(&mut self) {
        if self.mic.is_err() && self.mic_tried.elapsed() >= MIC_RETRY {
            self.mic = Mic::open().map_err(|e| return e.to_string());
            self.mic_tried = Instant::now();
        }
    }

    fn drain_mic(&mut self) {
        let Ok(mic) = &self.mic else {
            return;
        };
        let events: Vec<MicEvent> = mic.receiver().try_iter().collect();
        for event in events {
            match event {
                MicEvent::Samples(block) => self.audio(&block),
                MicEvent::Lost(error) => self.mic_lost(error),
            }
        }
    }

    fn audio(&mut self, block: &[i16]) {
        self.level = f32::from(block.iter().map(|s| return s.unsigned_abs()).max().unwrap_or(0)) / 32_768.0;
        match &mut self.screen {
            Screen::Takes(takes) => {
                if let Some(segment) = takes.ptt.feed(block) {
                    self.save_segment(segment);
                }
            }
            Screen::Background(background) => {
                let Some(chunker) = background.chunker.as_mut() else {
                    return;
                };
                let chunks = chunker.feed(block);
                let done = chunker.is_done();
                chunks.iter().for_each(|chunk| return self.save_chunk(chunk));
                if done {
                    self.stop_background();
                }
            }
            Screen::Setup(_) | Screen::Summary(_) => {}
        }
    }

    fn mic_lost(&mut self, error: String) {
        self.mic = Err(format!("Microphone lost ({error}). Reconnect the controller."));
        self.mic_tried = Instant::now();
        match &mut self.screen {
            Screen::Takes(takes) => {
                takes.ptt = PushToTalk::new(takes.rate);
                takes.session.handle(Input::Up(Button::R1), Instant::now());
                takes.recording = false;
            }
            Screen::Background(_) => self.stop_background(),
            Screen::Setup(_) | Screen::Summary(_) => {}
        }
    }

    fn handle(&mut self, input: Input) {
        let effects = match &mut self.screen {
            Screen::Setup(setup) => {
                let Input::Down(button) = input else {
                    return;
                };
                if let Some(start) = setup.handle(button) {
                    self.begin(start);
                }
                return;
            }
            Screen::Takes(takes) => takes.session.handle(input, Instant::now()),
            Screen::Background(background) => background.session.handle(input),
            Screen::Summary(_) => {
                if input == Input::Down(Button::Cross) {
                    self.screen = Screen::Setup(Setup::new(&self.plan));
                }
                return;
            }
        };
        effects.into_iter().for_each(|effect| return self.run(effect));
    }

    fn begin(&mut self, start: Start) {
        if let Some(reason) = self.not_ready() {
            self.message = Some(reason);
            return;
        }
        let rate = self.mic.as_ref().map_or(0, |mic| return mic.info().sample_rate);
        let id = Utc::now().format(TIME_IN_SESSION_ID).to_string();
        self.last = None;
        self.message = None;
        match start {
            Start::Takes(selection) => {
                let store = &self.store;
                let queue = pick(
                    &self.plan,
                    &store.progress(),
                    |cell| return store.is_removed(cell),
                    &selection,
                    &mut rand::rng(),
                );
                if queue.is_empty() {
                    self.message = Some("Nothing left to record for what is ticked.".to_string());
                    return;
                }
                self.screen = Screen::Takes(Takes {
                    session: TakeSession::new(queue),
                    ptt: PushToTalk::new(rate),
                    rate,
                    recording: false,
                    id,
                });
            }
            Start::Background(cell) => {
                self.screen = Screen::Background(Background {
                    session: BackgroundSession::new(cell),
                    chunker: None,
                    id,
                });
            }
        }
    }

    fn not_ready(&self) -> Option<String> {
        if let Err(reason) = &self.mic {
            return Some(reason.clone());
        }
        if !self.controller.is_connected() {
            return Some("No controller found. Plug the DualSense in over USB.".to_string());
        }
        return None;
    }

    fn run(&mut self, effect: Effect) {
        let now = Utc::now();
        let result = match effect {
            Effect::StartTake => {
                if let Screen::Takes(takes) = &mut self.screen {
                    takes.recording = true;
                    if let Some(previous) = takes.ptt.press() {
                        self.save_segment(previous);
                    }
                }
                Ok(())
            }
            Effect::StopTake => {
                if let Screen::Takes(takes) = &mut self.screen {
                    takes.recording = false;
                    takes.ptt.release();
                }
                Ok(())
            }
            Effect::StartBackground => {
                self.start_background();
                Ok(())
            }
            Effect::StopBackground => {
                self.stop_background();
                Ok(())
            }
            Effect::Reject(file) => {
                self.last = None;
                self.store.reject(&file, now).map_err(|e| return e.to_string())
            }
            Effect::Play(file) => read_wav(&self.store.root().join(file))
                .map_err(|e| return e.to_string())
                .and_then(|(samples, rate)| return play(&samples, rate))
                .map(|playback| self.playback = Some(playback)),
            Effect::Remove(cell) => self.store.remove(&cell, now).map_err(|e| return e.to_string()),
            Effect::End => {
                self.end();
                Ok(())
            }
        };
        if let Err(error) = result {
            self.message = Some(error);
        }
    }

    fn save_segment(&mut self, segment: Segment) {
        let (Screen::Takes(takes), Ok(mic)) = (&mut self.screen, &self.mic) else {
            return;
        };
        let Some(queued) = takes.session.current().cloned() else {
            return;
        };
        let found = warnings(queued.cell.kind, &segment);
        let volume = capture_volume(mic.info());
        let saved = self.store.save(NewTake {
            clip: Clip::Prompt(&queued.cell),
            samples: &segment.samples,
            style: Some(&queued.style),
            speaker: &self.speaker,
            session_id: &takes.id,
            time: Utc::now(),
            mic: mic.info(),
            capture_volume: volume.as_deref(),
            warnings: &found,
        });
        match saved {
            Ok(file) => {
                takes.session.saved(file);
                self.last = Some(LastTake {
                    waveform: waveform(&segment.samples),
                    warnings: found,
                });
                if takes.session.current().is_none() {
                    self.end();
                }
            }
            Err(error) => self.message = Some(format!("Take not saved: {error}")),
        }
    }

    fn start_background(&mut self) {
        let (Screen::Background(background), Ok(mic)) = (&mut self.screen, &self.mic) else {
            return;
        };
        let left = background
            .session
            .cell()
            .target
            .saturating_sub(self.store.progress().recorded(background.session.cell()));
        if left.is_zero() {
            background.session.stopped();
            self.message = Some("This condition has all its background already.".to_string());
            return;
        }
        background.chunker = Some(Chunker::new(mic.info().sample_rate, left));
    }

    fn stop_background(&mut self) {
        let Screen::Background(background) = &mut self.screen else {
            return;
        };
        background.session.stopped();
        if let Some(last) = background.chunker.take().and_then(Chunker::finish) {
            self.save_chunk(&last);
        }
    }

    fn save_chunk(&mut self, samples: &[i16]) {
        let (Screen::Background(background), Ok(mic)) = (&mut self.screen, &self.mic) else {
            return;
        };
        let volume = capture_volume(mic.info());
        let cell: BackgroundCell = background.session.cell().clone();
        let saved = self.store.save(NewTake {
            clip: Clip::Background(&cell),
            samples,
            style: None,
            speaker: &self.speaker,
            session_id: &background.id,
            time: Utc::now(),
            mic: mic.info(),
            capture_volume: volume.as_deref(),
            warnings: &[],
        });
        match saved {
            Ok(file) => {
                let length = Duration::from_millis(samples.len() as u64 * 1000 / u64::from(mic.info().sample_rate));
                background.session.chunk_saved(file, length);
                self.last = Some(LastTake {
                    waveform: waveform(samples),
                    warnings: Vec::new(),
                });
            }
            Err(error) => self.message = Some(format!("Background not saved: {error}")),
        }
    }

    fn end(&mut self) {
        let lines = match &self.screen {
            Screen::Takes(takes) => takes
                .session
                .summary()
                .iter()
                .map(|(condition, count)| return format!("{}: {count} takes", self.label(condition)))
                .collect(),
            Screen::Background(background) => vec![format!(
                "{}: {} of background",
                self.label(&background.session.cell().condition_id),
                minutes(background.session.recorded())
            )],
            Screen::Setup(_) | Screen::Summary(_) => return,
        };
        self.screen = Screen::Summary(lines);
    }

    fn label(&self, condition_id: &str) -> String {
        return self
            .plan
            .conditions()
            .iter()
            .find(|c| return c.id == condition_id)
            .map_or_else(|| return condition_id.to_string(), |c| return c.label.clone());
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.reconnect_mic();
        self.drain_mic();
        let inputs: Vec<Input> = self.controller.inputs().into_iter().chain(keyboard(&ctx)).collect();
        inputs.into_iter().for_each(|input| return self.handle(input));
        if let Screen::Takes(takes) = &mut self.screen {
            let effects = takes.session.tick(Instant::now());
            effects.into_iter().for_each(|effect| return self.run(effect));
        }
        egui::CentralPanel::default().show(ui, |ui| return self.draw(ui));
        ctx.request_repaint_after(Duration::from_millis(15));
    }
}

impl App {
    fn draw(&mut self, ui: &mut egui::Ui) {
        match &self.screen {
            Screen::Setup(_) => self.draw_setup(ui),
            Screen::Takes(_) => self.draw_takes(ui),
            Screen::Background(_) => self.draw_background(ui),
            Screen::Summary(lines) => {
                ui.heading("Session done");
                lines.iter().for_each(|line| return drop(ui.label(line)));
                ui.label(format!("Still to record overall: {} takes", self.takes_left()));
                ui.add_space(12.0);
                ui.label("✕: back to the start screen");
            }
        }
        if let Some(message) = &self.message {
            ui.add_space(12.0);
            ui.colored_label(egui::Color32::from_rgb(220, 120, 40), message);
        }
    }

    fn takes_left(&self) -> u32 {
        let progress = self.store.progress();
        return self
            .plan
            .prompt_cells()
            .iter()
            .filter(|cell| return !self.store.is_removed(cell))
            .map(|cell| return cell.target.saturating_sub(progress.takes(cell)))
            .sum();
    }

    fn draw_setup(&mut self, ui: &mut egui::Ui) {
        let progress = self.store.progress();
        ui.heading("Where are you right now?");
        ui.horizontal(|ui| {
            ui.label("Speaker:");
            ui.text_edit_singleline(&mut self.speaker);
        });
        ui.add_space(8.0);
        let Screen::Setup(setup) = &self.screen else {
            return;
        };
        let ready = match &self.mic {
            Err(reason) => Some(reason.clone()),
            Ok(_) if !self.controller.is_connected() => Some("No controller found.".to_string()),
            Ok(_) => None,
        };
        for row in setup.rows() {
            let pointer = if setup.cursor() == row { "▶ " } else { "  " };
            let tick = if setup.is_ticked(row) { "☑" } else { "☐" };
            let text = match row {
                Row::Condition(i) => {
                    let condition = &self.plan.conditions()[i];
                    let cells = self
                        .plan
                        .prompt_cells()
                        .iter()
                        .filter(|c| return c.condition_id == condition.id);
                    let (kept, target) = cells.fold((0, 0), |(kept, target), cell| {
                        return (kept + progress.takes(cell).min(cell.target), target + cell.target);
                    });
                    let background = self
                        .plan
                        .background_cells()
                        .iter()
                        .find(|c| return c.condition_id == condition.id);
                    let minutes_text = background.map_or(String::new(), |cell| {
                        return format!(
                            " · background {} / {}",
                            minutes(progress.recorded(cell)),
                            minutes(cell.target)
                        );
                    });
                    format!("{tick} {}   {kept} / {target} takes{minutes_text}", condition.label)
                }
                Row::Kind(kind) => format!("{tick} {}", kind_label(kind)),
                Row::Background => format!("{tick} Background only (hands-free; tick exactly one condition)"),
                Row::Length => format!("Session length:  ◀ {} takes ▶", setup.length()),
                Row::Start => match (&ready, setup.start()) {
                    (Some(reason), _) => format!("Start — not possible: {reason}"),
                    (None, None) => "Start — tick a condition and something to record".to_string(),
                    (None, Some(_)) => "Start (✕)".to_string(),
                },
            };
            if matches!(row, Row::Kind(PromptKind::Command) | Row::Background | Row::Length) {
                ui.add_space(6.0);
            }
            ui.label(egui::RichText::new(format!("{pointer}{text}")).size(18.0).monospace());
        }
        ui.add_space(8.0);
        let total: u32 = self.plan.prompt_cells().iter().map(|cell| return cell.target).sum();
        ui.label(format!(
            "Overall: {} of {total} takes still to record",
            self.takes_left()
        ));
        ui.label("D-pad: move · ✕: tick · ◀ ▶: session length · (keyboard: arrows, Enter)");
    }

    fn draw_takes(&self, ui: &mut egui::Ui) {
        let Screen::Takes(takes) = &self.screen else {
            return;
        };
        let (number, total) = takes.session.position();
        let Some(queued) = takes.session.current() else {
            return;
        };
        ui.label(format!(
            "Take {number} / {total} · {}",
            self.label(&queued.cell.condition_id)
        ));
        if takes.session.is_paused() {
            ui.heading("Paused — OPTIONS: resume · ✕: end session");
            return;
        }
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label("Say:");
            ui.label(egui::RichText::new(prompt_text(&queued.cell.text)).size(56.0).strong());
            ui.label(egui::RichText::new(format!("Style: {}", queued.style)).size(22.0));
            ui.add_space(16.0);
            let recording = if takes.recording {
                "● Recording"
            } else {
                "Hold R1 to speak"
            };
            ui.label(egui::RichText::new(recording).size(24.0));
            ui.add(egui::ProgressBar::new(self.level).desired_width(400.0));
        });
        self.draw_last(ui);
        ui.add_space(12.0);
        ui.label("○ redo last take · △ play back · □ skip (hold 1 s: never ask again) · OPTIONS: pause");
        ui.label("(keyboard: Space = R1, Backspace = ○, P = △, S = □, Esc = OPTIONS)");
    }

    fn draw_background(&self, ui: &mut egui::Ui) {
        let Screen::Background(background) = &self.screen else {
            return;
        };
        let cell = background.session.cell();
        let left = cell.target.saturating_sub(self.store.progress().recorded(cell));
        ui.heading(format!("Background: {}", self.label(&cell.condition_id)));
        ui.label(format!(
            "Kept this session: {} · still needed: {}",
            minutes(background.session.recorded()),
            minutes(left)
        ));
        ui.add_space(16.0);
        let state = if background.session.is_recording() {
            "● Recording — R1 to stop"
        } else {
            "R1 to start"
        };
        ui.label(egui::RichText::new(state).size(28.0));
        ui.add(egui::ProgressBar::new(self.level).desired_width(400.0));
        self.draw_last(ui);
        ui.label("○ drop the last piece · △ play it · OPTIONS: end (while stopped)");
    }

    fn draw_last(&self, ui: &mut egui::Ui) {
        let Some(last) = &self.last else {
            return;
        };
        ui.add_space(12.0);
        ui.label("Last take:");
        let (rect, _) = ui.allocate_exact_size(egui::vec2(WAVEFORM_COLUMNS as f32 * 2.0, 80.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let stroke = egui::Stroke::new(2.0, ui.visuals().text_color());
        for (i, (low, high)) in last.waveform.iter().enumerate() {
            let x = rect.left() + i as f32 * 2.0;
            painter.line_segment(
                [
                    egui::pos2(x, rect.center().y - high * 40.0),
                    egui::pos2(x, rect.center().y - low * 40.0),
                ],
                stroke,
            );
        }
        if !last.warnings.is_empty() {
            let names: Vec<&str> = last.warnings.iter().map(|w| return warning_text(*w)).collect();
            ui.colored_label(
                egui::Color32::from_rgb(220, 120, 40),
                format!("Check: {}", names.join(", ")),
            );
        }
    }
}

fn waveform(samples: &[i16]) -> Vec<(f32, f32)> {
    let per_column = samples.len().div_ceil(WAVEFORM_COLUMNS).max(1);
    return samples
        .chunks(per_column)
        .map(|chunk| {
            let low = chunk.iter().copied().min().unwrap_or(0);
            let high = chunk.iter().copied().max().unwrap_or(0);
            return (f32::from(low) / 32_768.0, f32::from(high) / 32_768.0);
        })
        .collect();
}

fn minutes(duration: Duration) -> String {
    let seconds = duration.as_secs();
    return format!("{}:{:02}", seconds / 60, seconds % 60);
}

fn kind_label(kind: PromptKind) -> &'static str {
    return match kind {
        PromptKind::Command => "Commands",
        PromptKind::WakeWord => "Wake word (Hommel)",
        PromptKind::Phrase => "Phrases (Hommel, flip …)",
        PromptKind::NearMiss => "Near-misses (words that are not commands)",
    };
}

fn prompt_text(text: &str) -> String {
    return if text == "any short sentence" {
        "Any short sentence, Dutch or English".to_string()
    } else {
        format!("“{text}”")
    };
}

fn warning_text(warning: Warning) -> &'static str {
    return match warning {
        Warning::Clipped => "too loud (clipped)",
        Warning::VeryQuiet => "very quiet",
        Warning::TooShort => "R1 released very quickly",
        Warning::TooLong => "longer than 4 s",
    };
}
