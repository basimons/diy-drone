use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use drone_voice::{
    BackgroundCell, Clip, MicInfo, NewTake, PromptCell, PromptKind, Store, StoreError, Warning, read_wav,
};

const HEADER: &str = "time_utc,session_id,event,file,kind,prompt_id,prompt_text,condition_id,style,speaker,\
                      duration_ms,sample_rate,channels,device_name,capture_volume,peak,warnings";

fn flip() -> PromptCell {
    return cell("flip");
}

fn cell(prompt_id: &str) -> PromptCell {
    return PromptCell {
        kind: PromptKind::Command,
        prompt_id: prompt_id.to_string(),
        text: prompt_id.to_string(),
        condition_id: "quiet-room".to_string(),
        target: 12,
    };
}

fn mic() -> MicInfo {
    return MicInfo {
        device_name: "DualSense Wireless Controller".to_string(),
        sample_rate: 48_000,
        channels: 2,
    };
}

fn at(millis: i64) -> DateTime<Utc> {
    return Utc
        .timestamp_millis_opt(
            Utc.with_ymd_and_hms(2026, 10, 29, 14, 3, 12)
                .unwrap()
                .timestamp_millis()
                + millis,
        )
        .unwrap();
}

fn take<'a>(clip: Clip<'a>, samples: &'a [i16], time: DateTime<Utc>, mic: &'a MicInfo) -> NewTake<'a> {
    return NewTake {
        clip,
        samples,
        style: Some("flat / tired"),
        speaker: "Bram S.",
        session_id: "s1",
        time,
        mic,
        capture_volume: Some("80%"),
        warnings: &[],
    };
}

fn save_flips(store: &mut Store, count: i64) -> Vec<std::path::PathBuf> {
    let mic = mic();
    let samples = vec![100_i16; 4_800];
    return (0..count)
        .map(|i| return store.save(take(Clip::Prompt(&flip()), &samples, at(i), &mic)).unwrap())
        .collect();
}

#[test]
fn a_saved_take_is_a_mono_wav_at_its_documented_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let samples: Vec<i16> = (0..4_800).map(|i| return (i % 200) as i16 - 100).collect();
    let file = store
        .save(take(Clip::Prompt(&flip()), &samples, at(345), &mic()))
        .unwrap();
    assert_eq!(
        file,
        Path::new("takes/command/flip/quiet-room/bram-s__2026-10-29T14-03-12.345Z__flat-tired.wav")
    );
    assert_eq!(read_wav(&dir.path().join(&file)).unwrap(), (samples, 48_000));
}

#[test]
fn progress_counts_takes_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    save_flips(&mut store, 3);
    assert_eq!(store.progress().takes(&flip()), 3);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.progress().takes(&flip()), 3);
    assert_eq!(store.last_speaker(), Some("Bram S."));
}

#[test]
fn a_fresh_store_has_no_speaker_and_no_progress() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("new")).unwrap();
    assert_eq!(store.last_speaker(), None);
    assert_eq!(store.progress().takes(&flip()), 0);
}

#[test]
fn a_rejected_take_stops_counting_and_moves_aside() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let files = save_flips(&mut store, 2);
    store.reject(&files[0], at(10_000)).unwrap();
    assert_eq!(store.progress().takes(&flip()), 1);
    assert!(!dir.path().join(&files[0]).exists());
    assert!(dir.path().join("rejected").join(&files[0]).exists());
    drop(store);
    assert_eq!(Store::open(dir.path()).unwrap().progress().takes(&flip()), 1);
}

#[test]
fn rejecting_an_unknown_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let files = save_flips(&mut store, 1);
    store.reject(&files[0], at(1)).unwrap();
    assert!(matches!(
        store.reject(&files[0], at(2)),
        Err(StoreError::UnknownFile(_))
    ));
    assert!(matches!(
        store.reject(Path::new("takes/nope.wav"), at(3)),
        Err(StoreError::UnknownFile(_))
    ));
}

#[test]
fn background_progress_sums_durations() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let cell = BackgroundCell {
        condition_id: "fan".to_string(),
        target: Duration::from_secs(300),
    };
    let (mic, long, short) = (mic(), vec![0_i16; 48_000], vec![0_i16; 24_000]);
    let first = store
        .save(NewTake {
            style: None,
            ..take(Clip::Background(&cell), &long, at(0), &mic)
        })
        .unwrap();
    store
        .save(NewTake {
            style: None,
            ..take(Clip::Background(&cell), &short, at(1), &mic)
        })
        .unwrap();
    assert_eq!(store.progress().recorded(&cell), Duration::from_millis(1_500));
    assert_eq!(
        first,
        Path::new("takes/background/background/fan/bram-s__2026-10-29T14-03-12.000Z__none.wav")
    );
}

#[test]
fn removed_cells_persist() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    assert!(!store.is_removed(&flip()));
    store.remove(&flip(), at(0)).unwrap();
    assert!(store.is_removed(&flip()));
    assert!(!store.is_removed(&cell("land")));
    drop(store);
    assert!(Store::open(dir.path()).unwrap().is_removed(&flip()));
    let removed = fs::read_to_string(dir.path().join("removed.csv")).unwrap();
    assert_eq!(
        removed,
        "prompt_id,condition_id,time_utc\nflip,quiet-room,2026-10-29T14:03:12.000Z\n"
    );
}

#[test]
fn a_truncated_last_manifest_line_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    save_flips(&mut store, 2);
    drop(store);
    fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("manifest.csv"))
        .unwrap()
        .write_all(b"2026-10-29T1")
        .unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.progress().takes(&flip()), 2);
    let samples = vec![1_i16; 10];
    store
        .save(take(Clip::Prompt(&flip()), &samples, at(99), &mic()))
        .unwrap();
    drop(store);
    assert_eq!(Store::open(dir.path()).unwrap().progress().takes(&flip()), 3);
}

#[test]
fn a_broken_line_in_the_middle_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    save_flips(&mut store, 1);
    drop(store);
    let path = dir.path().join("manifest.csv");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}garbage\n{}", text.lines().nth(1).unwrap())).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Csv(_))));
}

#[test]
fn part_files_are_not_takes() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("takes/command/flip/quiet-room/x.wav.part");
    fs::create_dir_all(part.parent().unwrap()).unwrap();
    fs::write(&part, b"half a wav").unwrap();
    assert_eq!(Store::open(dir.path()).unwrap().progress().takes(&flip()), 0);
}

#[test]
fn manifest_rows_for_prompts_no_longer_in_the_plan_are_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let samples = vec![1_i16; 10];
    store
        .save(take(Clip::Prompt(&cell("old-word")), &samples, at(0), &mic()))
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.progress().takes(&flip()), 0);
    assert_eq!(store.progress().takes(&cell("old-word")), 1);
}

#[test]
fn manifest_has_the_documented_columns() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let samples = [5_i16, -32_768, 7];
    let warnings = [Warning::Clipped, Warning::TooLong];
    let file = store
        .save(NewTake {
            warnings: &warnings,
            ..take(Clip::Prompt(&flip()), &samples, at(0), &mic())
        })
        .unwrap();
    let manifest = fs::read_to_string(dir.path().join("manifest.csv")).unwrap();
    let lines: Vec<&str> = manifest.lines().collect();
    assert_eq!(lines[0], HEADER);
    assert_eq!(
        lines[1],
        format!(
            "2026-10-29T14:03:12.000Z,s1,take,{},command,flip,flip,quiet-room,flat / tired,Bram S.,0,48000,2,\
             DualSense Wireless Controller,80%,32768,clipped;too_long",
            file.display()
        )
    );
}
