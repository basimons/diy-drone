//! The window: runs the tested state machines against the real mic, controller, store and speakers.

use std::time::{Duration, Instant};

use chrono::Utc;
use drone_voice::{
    BackgroundCell, Chunker, Clip, Mic, MicEvent, MicInfo, NewTake, Plan, PromptKind, PushToTalk, Segment, Store,
    Warning, capture_volume, pick, read_wav, warnings,
};
use eframe::egui::{self, RichText};
use voice_recorder::{BackgroundSession, Button, Effect, Input, Row, Setup, Start, TakeSession};

use crate::input::{Controller, keyboard};
use crate::look;
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
    mic: MicInfo,           // as at the session's start, so saving never depends on a live mic
    volume: Option<String>, // capture volume at the session's start
    id: String,
}

struct Background {
    session: BackgroundSession,
    chunker: Option<Chunker>, // Some while recording
    mic: MicInfo,
    volume: Option<String>,
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
                takes.ptt = PushToTalk::new(takes.mic.sample_rate);
                takes.session.not_saved();
            }
            Screen::Background(_) => self.stop_background(),
            Screen::Setup(_) | Screen::Summary(_) => {}
        }
    }

    fn handle(&mut self, input: Input) {
        let recording_screen = matches!(self.screen, Screen::Takes(_) | Screen::Background(_));
        if recording_screen && input == Input::Down(Button::R1) && self.mic.is_err() {
            self.message = Some("No microphone: reconnect the controller over USB.".to_string());
            return;
        }
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
        let mic = match (&self.mic, self.controller.is_connected()) {
            (Err(reason), _) => {
                self.message = Some(reason.clone());
                return;
            }
            (Ok(_), false) => {
                self.message = Some("No controller found. Plug the DualSense in over USB.".to_string());
                return;
            }
            (Ok(mic), true) => mic.info().clone(),
        };
        let volume = capture_volume(&mic);
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
                    ptt: PushToTalk::new(mic.sample_rate),
                    mic,
                    volume,
                    id,
                });
            }
            Start::Background(cell) => {
                self.screen = Screen::Background(Background {
                    session: BackgroundSession::new(cell),
                    chunker: None,
                    mic,
                    volume,
                    id,
                });
            }
        }
    }

    fn run(&mut self, effect: Effect) {
        let now = Utc::now();
        let result = match effect {
            Effect::StartTake => {
                if let Screen::Takes(takes) = &mut self.screen
                    && let Some(previous) = takes.ptt.press()
                {
                    self.save_segment(previous);
                }
                Ok(())
            }
            Effect::StopTake => {
                if let Screen::Takes(takes) = &mut self.screen {
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
        let Screen::Takes(takes) = &mut self.screen else {
            return;
        };
        let Some(queued) = takes.session.current().cloned() else {
            debug_assert!(false, "a take finished with no prompt to save it under");
            return;
        };
        let found = warnings(queued.cell.kind, &segment);
        let saved = self.store.save(NewTake {
            clip: Clip::Prompt(&queued.cell),
            samples: &segment.samples,
            style: Some(&queued.style),
            speaker: &self.speaker,
            session_id: &takes.id,
            time: Utc::now(),
            mic: &takes.mic,
            capture_volume: takes.volume.as_deref(),
            warnings: &found,
        });
        match saved {
            Ok(file) => {
                takes.session.saved(file);
                self.last = Some(LastTake {
                    waveform: waveform(&segment.samples),
                    warnings: found,
                });
            }
            Err(error) => {
                takes.session.not_saved();
                self.message = Some(format!("Take not saved: {error}"));
            }
        }
    }

    fn start_background(&mut self) {
        let Screen::Background(background) = &mut self.screen else {
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
        background.chunker = Some(Chunker::new(background.mic.sample_rate, left));
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
        let Screen::Background(background) = &mut self.screen else {
            return;
        };
        let cell: BackgroundCell = background.session.cell().clone();
        let saved = self.store.save(NewTake {
            clip: Clip::Background(&cell),
            samples,
            style: None,
            speaker: &self.speaker,
            session_id: &background.id,
            time: Utc::now(),
            mic: &background.mic,
            capture_volume: background.volume.as_deref(),
            warnings: &[],
        });
        match saved {
            Ok(file) => {
                let length = Duration::from_millis(samples.len() as u64 * 1000 / u64::from(background.mic.sample_rate));
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
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(32, 24))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    match &self.screen {
                        Screen::Setup(_) => self.draw_setup(ui),
                        Screen::Takes(_) => self.draw_takes(ui),
                        Screen::Background(_) => self.draw_background(ui),
                        Screen::Summary(lines) => self.draw_summary(ui, lines),
                    }
                    if let Some(message) = &self.message {
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("⚠ {message}")).color(look::WARNING));
                    }
                });
            });
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

    fn overall(&self, ui: &mut egui::Ui) {
        let total: u32 = self.plan.prompt_cells().iter().map(|cell| return cell.target).sum();
        let done = total - self.takes_left().min(total);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Overall").color(look::WEAK));
            ui.add(
                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                    .desired_width(320.0)
                    .desired_height(8.0)
                    .fill(look::ACCENT),
            );
            ui.label(format!("{done} / {total} takes"));
        });
    }

    fn draw_setup(&mut self, ui: &mut egui::Ui) {
        let progress = self.store.progress();
        ui.heading(RichText::new("Where are you right now?").strong());
        ui.label(
            RichText::new("Tick what you can do now; the app only asks for what is still missing there.")
                .color(look::WEAK),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Speaker");
            ui.add(egui::TextEdit::singleline(&mut self.speaker).desired_width(220.0));
        });
        let Screen::Setup(setup) = &self.screen else {
            return;
        };
        let ready = match &self.mic {
            Err(reason) => Some(reason.clone()),
            Ok(_) if !self.controller.is_connected() => Some("No controller found.".to_string()),
            Ok(_) => None,
        };
        for row in setup.rows() {
            match row {
                Row::Condition(0) => look::section(ui, "Conditions"),
                Row::Kind(PromptKind::Command) => look::section(ui, "What to record"),
                Row::Length => look::section(ui, "Session"),
                _ => {}
            }
            let selected = setup.cursor() == row;
            let tick = |ui: &mut egui::Ui| {
                let (glyph, color) = if setup.is_ticked(row) {
                    ("☑", look::ACCENT)
                } else {
                    ("☐", look::WEAK)
                };
                ui.label(RichText::new(glyph).size(22.0).color(color));
            };
            match row {
                Row::Condition(i) => look::card(ui, selected, |ui| {
                    let condition = &self.plan.conditions()[i];
                    let (kept, target) = self
                        .plan
                        .prompt_cells()
                        .iter()
                        .filter(|c| return c.condition_id == condition.id)
                        .fold((0, 0), |(kept, target), cell| {
                            return (kept + progress.takes(cell).min(cell.target), target + cell.target);
                        });
                    let background = self
                        .plan
                        .background_cells()
                        .iter()
                        .find(|c| return c.condition_id == condition.id);
                    ui.horizontal(|ui| {
                        tick(ui);
                        ui.label(&condition.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(cell) = background {
                                let text = format!(
                                    "background {} / {}",
                                    minutes(progress.recorded(cell)),
                                    minutes(cell.target)
                                );
                                ui.label(RichText::new(text).small().color(look::WEAK));
                            }
                            if target > 0 {
                                ui.label(RichText::new(format!("{kept} / {target}")).color(look::TEXT));
                                ui.add(
                                    egui::ProgressBar::new(kept as f32 / target as f32)
                                        .desired_width(160.0)
                                        .desired_height(8.0)
                                        .fill(look::ACCENT),
                                );
                            }
                        });
                    });
                }),
                Row::Kind(kind) => look::card(ui, selected, |ui| {
                    ui.horizontal(|ui| {
                        tick(ui);
                        ui.label(kind_label(kind));
                    });
                }),
                Row::Background => look::card(ui, selected, |ui| {
                    ui.horizontal(|ui| {
                        tick(ui);
                        ui.label("Background only");
                        ui.label(RichText::new("hands-free; tick exactly one condition").color(look::WEAK));
                    });
                }),
                Row::Length => look::card(ui, selected, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Length");
                        ui.label(RichText::new(format!("◀  {} takes  ▶", setup.length())).strong());
                        ui.label(RichText::new(format!("≈ {} min", setup.length() / 8)).color(look::WEAK));
                    });
                }),
                Row::Start => {
                    ui.add_space(8.0);
                    let (text, fill) = match (&ready, setup.start()) {
                        (Some(reason), _) => (format!("Can't start: {reason}"), look::WEAK),
                        (None, None) => ("Tick a condition and something to record".to_string(), look::WEAK),
                        (None, Some(_)) => ("✕  Start".to_string(), look::ACCENT),
                    };
                    let stroke = if selected {
                        egui::Stroke::new(3.0, look::TEXT)
                    } else {
                        egui::Stroke::NONE
                    };
                    egui::Frame::new()
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(18, 10))
                        .show(ui, |ui| {
                            ui.label(RichText::new(text).size(20.0).strong().color(egui::Color32::BLACK));
                        });
                }
            }
        }
        ui.add_space(14.0);
        self.overall(ui);
        ui.add_space(6.0);
        look::hints(
            ui,
            &[
                (look::DPAD, "move"),
                (look::CROSS, "tick / start"),
                (("◀ ▶", look::TEXT), "length"),
            ],
        );
        ui.label(RichText::new("Keyboard: arrows, Enter").small().color(look::WEAK));
    }

    fn draw_takes(&self, ui: &mut egui::Ui) {
        let Screen::Takes(takes) = &self.screen else {
            return;
        };
        let (number, total) = takes.session.position();
        let Some(queued) = takes.session.current() else {
            ui.heading(RichText::new("All done for this session").strong());
            ui.add_space(8.0);
            self.draw_last(ui);
            ui.add_space(12.0);
            look::hints(
                ui,
                &[
                    (look::CIRCLE, "redo the last take"),
                    (look::TRIANGLE, "play it"),
                    (look::CROSS, "finish"),
                ],
            );
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Take {number} of {total}")).color(look::WEAK));
            look::chip(ui, &self.label(&queued.cell.condition_id), look::WEAK);
            ui.add(
                egui::ProgressBar::new((number - 1) as f32 / total.max(1) as f32)
                    .desired_width(200.0)
                    .fill(look::ACCENT),
            );
        });
        ui.add_space(20.0);
        if takes.session.is_paused() {
            look::card(ui, true, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.heading(RichText::new("Paused").strong());
                    ui.add_space(10.0);
                    look::hints(ui, &[(look::OPTIONS, "resume"), (look::CROSS, "end the session")]);
                    ui.add_space(30.0);
                });
            });
            return;
        }
        let recording = takes.session.is_recording();
        let frame_color = if recording {
            look::RECORDING
        } else {
            egui::Color32::TRANSPARENT
        };
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(36, 36, 42))
            .stroke(egui::Stroke::new(3.0, frame_color))
            .corner_radius(16)
            .inner_margin(egui::Margin::symmetric(20, 28))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Say").color(look::WEAK));
                    ui.label(RichText::new(prompt_text(&queued.cell.text)).size(64.0).strong());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let width = 80.0 + queued.style.len() as f32 * 9.0;
                        ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                        ui.label(RichText::new("how:").color(look::WEAK));
                        look::chip(ui, &queued.style, look::ACCENT);
                    });
                    ui.add_space(18.0);
                    let (state, color) = if recording {
                        ("●  Recording", look::RECORDING)
                    } else {
                        ("Hold R1 to speak", look::TEXT)
                    };
                    ui.label(RichText::new(state).size(26.0).strong().color(color));
                    look::meter(ui, self.level, recording);
                });
            });
        self.draw_last(ui);
        ui.add_space(12.0);
        look::hints(
            ui,
            &[
                (look::R1, "hold to speak"),
                (look::CIRCLE, "redo"),
                (look::TRIANGLE, "play back"),
                (look::SQUARE, "skip (hold 1 s: never ask again)"),
                (look::OPTIONS, "pause"),
            ],
        );
        ui.label(
            RichText::new("Keyboard: Space = R1, Backspace = ○, P = △, S = □, Esc = OPTIONS")
                .small()
                .color(look::WEAK),
        );
    }

    fn draw_background(&self, ui: &mut egui::Ui) {
        let Screen::Background(background) = &self.screen else {
            return;
        };
        let cell = background.session.cell();
        let left = cell.target.saturating_sub(self.store.progress().recorded(cell));
        let recording = background.session.is_recording();
        ui.heading(RichText::new(format!("Background: {}", self.label(&cell.condition_id))).strong());
        ui.label(RichText::new("Put the controller down and let it listen. Don't talk to it.").color(look::WEAK));
        ui.add_space(16.0);
        let frame_color = if recording {
            look::RECORDING
        } else {
            egui::Color32::TRANSPARENT
        };
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(36, 36, 42))
            .stroke(egui::Stroke::new(3.0, frame_color))
            .corner_radius(16)
            .inner_margin(egui::Margin::symmetric(20, 28))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let (state, color) = if recording {
                        ("●  Recording", look::RECORDING)
                    } else {
                        ("Press R1 to start", look::TEXT)
                    };
                    ui.label(RichText::new(state).size(30.0).strong().color(color));
                    ui.label(format!(
                        "kept this session {}  ·  still needed {}",
                        minutes(background.session.recorded()),
                        minutes(left)
                    ));
                    look::meter(ui, self.level, recording);
                });
            });
        self.draw_last(ui);
        ui.add_space(12.0);
        look::hints(
            ui,
            &[
                (look::R1, "start / stop"),
                (look::CIRCLE, "drop the last piece"),
                (look::TRIANGLE, "play it"),
                (look::OPTIONS, "end (while stopped)"),
            ],
        );
    }

    fn draw_summary(&self, ui: &mut egui::Ui, lines: &[String]) {
        ui.heading(RichText::new("Session done").strong());
        ui.add_space(10.0);
        look::card(ui, false, |ui| {
            lines
                .iter()
                .for_each(|line| return drop(ui.label(RichText::new(line).size(20.0))));
        });
        ui.add_space(12.0);
        self.overall(ui);
        ui.add_space(12.0);
        look::hints(ui, &[(look::CROSS, "back to the start screen")]);
    }

    fn draw_last(&self, ui: &mut egui::Ui) {
        let Some(last) = &self.last else {
            return;
        };
        ui.add_space(12.0);
        look::card(ui, false, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Last take").color(look::WEAK));
                let names: Vec<&str> = last.warnings.iter().map(|w| return warning_text(*w)).collect();
                if names.is_empty() {
                    ui.label(RichText::new("✔ looks fine").color(look::WEAK));
                } else {
                    ui.label(RichText::new(format!("⚠ {}", names.join(", "))).color(look::WARNING));
                }
            });
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            let step = rect.width() / WAVEFORM_COLUMNS as f32;
            let stroke = egui::Stroke::new(step.max(1.0) * 0.7, look::ACCENT);
            for (i, (low, high)) in last.waveform.iter().enumerate() {
                let x = rect.left() + (i as f32 + 0.5) * step;
                // Scaled up 4×: takes rarely come near full scale, and the shape matters more than the level here.
                let (top, bottom) = ((high * 4.0).min(1.0), (low * 4.0).max(-1.0));
                painter.line_segment(
                    [
                        egui::pos2(x, rect.center().y - top * 34.0),
                        egui::pos2(x, rect.center().y - bottom * 34.0 + 1.0),
                    ],
                    stroke,
                );
            }
        });
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
