//! Controller (gilrs) and keyboard (egui) events as recorder inputs. The keyboard mirrors the controller so the app
//! can be tried without one.

use eframe::egui;
use voice_recorder::{Button, Input};

pub struct Controller {
    gilrs: Option<gilrs::Gilrs>,
}

impl Controller {
    pub fn new() -> Controller {
        return Controller {
            gilrs: gilrs::Gilrs::new().ok(),
        };
    }

    pub fn is_connected(&self) -> bool {
        return self
            .gilrs
            .as_ref()
            .is_some_and(|gilrs| return gilrs.gamepads().next().is_some());
    }

    /// Button events since the last call.
    pub fn inputs(&mut self) -> Vec<Input> {
        let Some(gilrs) = self.gilrs.as_mut() else {
            return Vec::new();
        };
        return std::iter::from_fn(|| return gilrs.next_event())
            .filter_map(|event| {
                return match event.event {
                    gilrs::EventType::ButtonPressed(button, _) => gamepad_button(button).map(Input::Down),
                    gilrs::EventType::ButtonReleased(button, _) => gamepad_button(button).map(Input::Up),
                    _ => None,
                };
            })
            .collect();
    }
}

fn gamepad_button(button: gilrs::Button) -> Option<Button> {
    return match button {
        gilrs::Button::RightTrigger => Some(Button::R1),
        gilrs::Button::South => Some(Button::Cross),
        gilrs::Button::East => Some(Button::Circle),
        gilrs::Button::North => Some(Button::Triangle),
        gilrs::Button::West => Some(Button::Square),
        gilrs::Button::Start => Some(Button::Options),
        gilrs::Button::DPadUp => Some(Button::Up),
        gilrs::Button::DPadDown => Some(Button::Down),
        gilrs::Button::DPadLeft => Some(Button::Left),
        gilrs::Button::DPadRight => Some(Button::Right),
        _ => None,
    };
}

/// Keyboard events of this frame, unless a text field has the focus (then the keys are typing).
pub fn keyboard(ctx: &egui::Context) -> Vec<Input> {
    if ctx.memory(|memory| return memory.focused().is_some()) {
        return Vec::new();
    }
    return ctx.input(|input| {
        return input
            .events
            .iter()
            .filter_map(|event| {
                let egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    ..
                } = event
                else {
                    return None;
                };
                let button = key_button(*key)?;
                return Some(if *pressed {
                    Input::Down(button)
                } else {
                    Input::Up(button)
                });
            })
            .collect();
    });
}

fn key_button(key: egui::Key) -> Option<Button> {
    return match key {
        egui::Key::Space => Some(Button::R1),
        egui::Key::Enter => Some(Button::Cross),
        egui::Key::Backspace => Some(Button::Circle),
        egui::Key::P => Some(Button::Triangle),
        egui::Key::S => Some(Button::Square),
        egui::Key::Escape => Some(Button::Options),
        egui::Key::ArrowUp => Some(Button::Up),
        egui::Key::ArrowDown => Some(Button::Down),
        egui::Key::ArrowLeft => Some(Button::Left),
        egui::Key::ArrowRight => Some(Button::Right),
        _ => None,
    };
}
