use crate::effects::{Effect, EffectSlot, Distortion, Bitcrusher, Delay, Chorus, Compressor, Reverb, Tremolo};
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize)]
pub enum PresetEffect {
    Distortion { drive: f32, hard: bool },
    Bitcrusher { bit_depth: u32 },
    Delay { delay_ms: f32, decay: f32, ping_pong: bool },
    Chorus { delay_ms: f32, depth_ms: f32, lfo_frequency: f32 },
    Compressor { threshold: f32, ratio: f32, attack_ms: f32, release_ms: f32 },
    Reverb { room_size: f32, decay: f32 },
    Tremolo { depth: f32, lfo_frequency: f32},
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PresetSlot {
    pub effect: PresetEffect,
    pub wet: f32,
    pub enabled: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    pub slots: Vec<PresetSlot>,
}

pub fn effects_to_preset(name: String, effects: &Vec<EffectSlot>) -> Preset {
    Preset {
        name,
        slots: effects.iter().map(|slot| PresetSlot {
            effect: slot.effect.to_preset(),
            wet: slot.wet,
            enabled: slot.enabled,
        }).collect(),
    }
}

pub fn preset_to_effects(preset: Preset) -> Vec<EffectSlot> {
    preset.slots.into_iter().map(|slot| {
        let effect: Box<dyn Effect + Send> = match slot.effect {
            PresetEffect::Distortion { drive, hard } => Box::new(Distortion { drive, hard }),
            PresetEffect::Bitcrusher { bit_depth } => Box::new(Bitcrusher { bit_depth }),
            PresetEffect::Delay { delay_ms, decay, ping_pong } => Box::new(Delay { delay_ms, decay, ping_pong, past_left_signal: Vec::new(), past_right_signal: Vec::new() }),
            PresetEffect::Chorus { delay_ms, depth_ms, lfo_frequency } => Box::new(Chorus { delay_ms, depth_ms, lfo_frequency, lfo_phase: 0.0, past_left_signal: Vec::new(), past_right_signal: Vec::new() }),
            PresetEffect::Compressor { threshold, ratio, attack_ms, release_ms } => Box::new(Compressor { threshold, ratio, attack_ms, release_ms, current_gain: 1.0 }),
            PresetEffect::Reverb { room_size, decay } => Box::new(Reverb::new(room_size, decay)),
            PresetEffect::Tremolo { depth, lfo_frequency} => Box::new(Tremolo { depth, lfo_frequency, lfo_phase: 0.0 }),
        };
        EffectSlot { effect, wet: slot.wet, enabled: slot.enabled }
    }).collect()
}

pub fn preset_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(".guitar_fx")
}