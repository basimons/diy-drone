//! `cargo run -p voice-recorder [-- --data <dir> --plan <file>]`: records the voice-command dataset.

mod app;
mod input;
mod look;
mod playback;

use std::path::PathBuf;

use anyhow::{Context, bail};
use drone_voice::{Plan, Store};
use eframe::egui;

fn main() -> anyhow::Result<()> {
    let (data, plan) = arguments()?;
    let plan = Plan::load(&plan).with_context(|| return format!("loading {}", plan.display()))?;
    let store = Store::open(&data).with_context(|| return format!("opening {}", data.display()))?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 960.0])
            .with_title("Voice recorder"),
        ..Default::default()
    };
    eframe::run_native(
        "voice-recorder",
        options,
        Box::new(|cc| {
            look::apply(&cc.egui_ctx);
            return Ok(Box::new(app::App::new(plan, store)));
        }),
    )
    .map_err(|e| return anyhow::anyhow!("window failed: {e}"))?;
    return Ok(());
}

/// `--data <dir>` (default `voice-data`) and `--plan <file>` (default `voice/plan.toml`).
fn arguments() -> anyhow::Result<(PathBuf, PathBuf)> {
    let mut data = PathBuf::from("voice-data");
    let mut plan = PathBuf::from("voice/plan.toml");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().with_context(|| return format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--data" => data = PathBuf::from(value),
            "--plan" => plan = PathBuf::from(value),
            _ => bail!("unknown argument {flag}; use --data <dir> and/or --plan <file>"),
        }
    }
    return Ok((data, plan));
}
