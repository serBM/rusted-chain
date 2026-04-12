mod effects;
mod preset;
mod ui;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::{Mutex, Arc};
use effects::{EffectSlot, SAMPLE_RATE, BUFFER_SIZE};

fn main() {
   // Preset loading
   std::fs::create_dir_all(crate::preset::preset_dir()).unwrap();
   for preset in std::env::current_dir().unwrap().join("presets").read_dir().unwrap() {
        let entry = preset.unwrap();
        let filename = entry.file_name();
        let destination = crate::preset::preset_dir().join(filename);
        std::fs::copy(entry.path(),destination)
            .expect("Failed to copy preset");
   }

    let host = cpal::default_host();

    let input_device = host
        .default_input_device()
        .expect("No input device found");

    let output_device = host
        .default_output_device()
        .expect("No output device found");

    println!("Input:  {}", input_device.name().unwrap());
    println!("Output: {}", output_device.name().unwrap());

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Fixed(BUFFER_SIZE),
    };

    // Ring buffer: holds 8x the buffer size to absorb timing differences between streams
    let latency_samples = BUFFER_SIZE as usize * config.channels as usize * 8;
    let rb = HeapRb::<f32>::new(latency_samples);
    let (mut producer, mut consumer) = rb.split();

    // Pre-fill with one buffer worth of silence
    for _ in 0..BUFFER_SIZE as usize * config.channels as usize {
        producer.try_push(0.0).ok();
    }

    // Effects definition with their settings
    let volume: Arc<Mutex<f32>> = Arc::new(Mutex::new(1.0));
    let volume_for_closure = Arc::clone(&volume);

    let global_wet: Arc<Mutex<f32>> = Arc::new(Mutex::new(1.0));
    let global_wet_for_closure = Arc::clone(&global_wet);

    let effects: Arc<Mutex<Vec<EffectSlot>>> = Arc::new(Mutex::new(Vec::new()));
    let effects_for_closure = Arc::clone(&effects);

    // Input stream: runs whenever new audio samples arrive from the microphone
    // `move` transfers ownership of `producer` into this closure
    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let vol = *volume_for_closure.lock().unwrap();
                let wet = *global_wet_for_closure.lock().unwrap();
                for chunk in data.chunks(2) { // chunk = [left, right]
                    let dry_left = chunk[0];
                    let dry_right = chunk[0];
                    let mut left_signal = dry_left;
                    let mut right_signal = dry_right;
                    for effect in effects_for_closure.lock().unwrap().iter_mut() {
                        if effect.enabled {
                            let (processed_left, processed_right) = effect.effect.process(left_signal, right_signal);
                            left_signal = (1.0 - effect.wet) * left_signal + effect.wet * processed_left;
                            right_signal = (1.0 - effect.wet) * right_signal + effect.wet * processed_right;
                        }
                    }
                    let out_left = (1.0 - wet) * dry_left + wet * left_signal;
                    let out_right = (1.0 - wet) * dry_right + wet * right_signal;
                    producer.try_push(out_left * vol).ok();
                    producer.try_push(out_right * vol).ok();
                }
            },
            |err| eprintln!("Input error: {}", err),
            None,
        )
        .expect("Failed to build input stream");

    // Output stream: runs whenever the audio hardware needs more samples to play
    // `move` transfers ownership of `consumer` into this closure
    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    *sample = consumer.try_pop().unwrap_or(0.0); // read from ring buffer, or silence
                }
            },
            |err| eprintln!("Output error: {}", err),
            None,
        )
        .expect("Failed to build output stream");

    // Start both streams
    input_stream.play().expect("Failed to start input stream");
    output_stream.play().expect("Failed to start output stream");

    println!("Audio passthrough running");

    ui::run_ui(effects, volume, global_wet);
}
