//! Real audio, so these are ignored by default and play quiet tones. Run from one shell, Windows:
//!   cargo test --test own_audio -- --ignored --exact other_app_tone &
//!   cargo test --test own_audio -- --ignored --exact own_voice_is_left_out
//! or, with the same tone running, `other_apps_turn_down_and_back`. That one briefly lowers
//! whatever else is playing, and puts it back.
#![cfg(windows)]

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hammurabi::audio::AudioCapture;
use hammurabi::audio::duck::{self, Ducker};
use hammurabi::tts::supertonic::play;
use hammurabi::types::AudioSource;

const RATE: u32 = 48_000;
const OWN: f32 = 1234.0;
const OTHER: f32 = 2000.0;

fn tone(hz: f32, secs: f32) -> Vec<f32> {
    (0..(RATE as f32 * secs) as usize)
        .map(|i| 0.05 * (i as f32 / RATE as f32 * hz * std::f32::consts::TAU).sin())
        .collect()
}

/// Amplitude of `hz` in `s` (Goertzel).
fn level(s: &[f32], rate: u32, hz: f32) -> f32 {
    let w = hz / rate as f32 * std::f32::consts::TAU;
    let (mut a, mut b) = (0.0, 0.0);
    for &x in s {
        (a, b) = (x + 2.0 * w.cos() * a - b, a);
    }
    (a * a + b * b - 2.0 * w.cos() * a * b).max(0.0).sqrt() * 2.0 / s.len().max(1) as f32
}

/// (own tone, other app's tone) levels captured while this process plays its tone.
fn capture_while_playing(exclude: bool) -> (f32, f32, bool) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let c = AudioCapture::start(AudioSource::System, None, None, exclude, tx).unwrap();
    let excluded = c.own_excluded;
    std::thread::sleep(Duration::from_millis(300));
    play(&tone(OWN, 1.5), RATE, None, &AtomicBool::new(false)).unwrap();
    drop(c);
    let (mut all, mut rate) = (Vec::new(), RATE);
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
        rate = chunk.sample_rate;
        all.extend(chunk.samples);
    }
    assert!(all.len() > rate as usize, "captured {} samples", all.len());
    (level(&all, rate, OWN), level(&all, rate, OTHER), excluded)
}

/// Stands in for another app; started separately so it is not this process's child.
#[test]
#[ignore]
fn other_app_tone() {
    play(&tone(OTHER, 9.0), RATE, None, &AtomicBool::new(false)).unwrap();
}

#[test]
#[ignore]
fn own_voice_is_left_out() {
    std::thread::sleep(Duration::from_secs(1));
    let (own, other, excluded) = capture_while_playing(false);
    println!("plain loopback: own {own:.5}, other {other:.5}");
    assert!(!excluded);
    assert!(own > 0.002, "plain loopback should hear us");

    let (own2, other2, excluded) = capture_while_playing(true);
    println!("own left out: own {own2:.5}, other {other2:.5}");
    assert!(excluded, "this Windows should support process loopback");
    assert!(own2 < own / 20.0, "our tone is still captured");
    assert!(other2 > 0.002, "the other app's tone is gone");
}

/// Level of the other app's tone over the next second.
fn other_level(rx: &crossbeam_channel::Receiver<hammurabi::types::AudioChunk>) -> f32 {
    while rx.try_recv().is_ok() {}
    let (mut all, mut rate) = (Vec::new(), RATE);
    while all.len() < RATE as usize {
        let chunk = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        rate = chunk.sample_rate;
        all.extend(chunk.samples);
    }
    level(&all, rate, OTHER)
}

#[test]
#[ignore]
fn other_apps_turn_down_and_back() {
    std::thread::sleep(Duration::from_secs(1));
    let (tx, rx) = crossbeam_channel::unbounded();
    let _c = AudioCapture::start(AudioSource::System, None, None, true, tx).unwrap();
    let before = other_level(&rx);
    println!("before {before:.5}");
    assert!(before > 0.002, "the other app's tone is not playing");

    let mut d = Ducker::default();
    d.duck(0.25);
    assert!(d.active());
    assert_eq!(d.gain(), 4.0);
    let low = other_level(&rx);
    println!("ducked {low:.5}");
    assert!((low / before - 0.25).abs() < 0.08, "{low} of {before}");

    d.release();
    assert!(!d.active());
    let back = other_level(&rx);
    println!("released {back:.5}");
    assert!((back / before - 1.0).abs() < 0.1, "{back} of {before}");

    // Dying with apps turned down: the next start puts them back.
    let mut d = Ducker::default();
    d.duck(0.25);
    std::mem::forget(d);
    assert!(other_level(&rx) / before < 0.4);
    duck::recover();
    let healed = other_level(&rx);
    println!("recovered {healed:.5}");
    assert!((healed / before - 1.0).abs() < 0.1, "{healed} of {before}");
}

/// A system voice read through the app's queue lands on the chosen device, not the default one.
#[test]
#[ignore]
fn voice_plays_on_the_chosen_device() {
    use hammurabi::audio::list_devices;
    use hammurabi::tts::{Engine, Speaker, Style};
    let devices = list_devices();
    let chosen = devices.outputs.iter().find(|d| !d.is_default).unwrap();
    println!("reading on {}", chosen.name);
    let energy = |output: Option<&str>| {
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = AudioCapture::start(AudioSource::System, None, output, false, tx).unwrap();
        let speaker = Speaker::spawn();
        speaker.configure(Style {
            persona: "custom".into(),
            voice: None,
            rate: 1.0,
            engine: Engine::System,
            neural_voice: "F3".into(),
            volume: 1.0,
            device: Some(chosen.id.clone()),
            duck: 1.0,
        });
        std::thread::sleep(Duration::from_millis(300));
        speaker.say("Testing the chosen device.".into(), "en".into());
        std::thread::sleep(Duration::from_secs(3));
        drop(speaker);
        drop(c);
        let mut peak = 0f32;
        while let Ok(chunk) = rx.try_recv() {
            peak = chunk.samples.iter().fold(peak, |p, s| p.max(s.abs()));
        }
        peak
    };
    let there = energy(Some(&chosen.id));
    println!("peak on the chosen device {there:.4}");
    assert!(there > 0.01, "nothing reached the chosen device");
}
