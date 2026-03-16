use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::{Mutex, Arc};

trait Effect {
    fn process(&mut self, signal: f32) -> f32;
    fn name(&self) -> &str;
}

struct Distortion {
    drive: f32,
    hard: bool,
}

impl Effect for Distortion {
    fn process(&mut self, signal: f32) -> f32 {
        if self.hard {
            (signal * self.drive).clamp(-1.0, 1.0)
        } else {
            (signal * self.drive).tanh()
        }
    }
    fn name(&self) -> &str {
        "distortion"
    }
}

struct Bitcrusher {
   bit_depth: u32,
}

impl Effect for Bitcrusher {
    fn process(&mut self, signal: f32) -> f32 {
        let steps = 2_f32.powi(self.bit_depth as i32);
        (signal * steps as f32).round() / steps as f32
    }
    fn name(&self) -> &str {
        "bitcrusher"
    }
}

struct Delay {
    past_signal: Vec<f32>,
    length: usize,
    decay: f32,
}

impl Effect for Delay {
    fn process(&mut self, signal: f32) -> f32 {
        // get delayed signal or 0.0 if past_signal is still empty
        let delayed = self.past_signal.get(0).copied().unwrap_or(0.0);
        // mix signal + delay
        let output: f32 = signal + (delayed * self.decay);
        // push current signal to the buffer
        self.past_signal.push(output);
        // clean past_signal buffer
        if self.past_signal.len() > self.length {
            self.past_signal.remove(0);
        }
        output
    }
    fn name(&self) -> &str {
        "delay"
    }
}

struct EffectSlot {
    effect: Box<dyn Effect + Send>,
    enabled: bool,
}

fn main() {
    let host = cpal::default_host();

    let input_device = host
        .default_input_device()
        .expect("No input device found");

    let output_device = host
        .default_output_device()
        .expect("No output device found");

    println!("Input:  {}", input_device.name().unwrap());
    println!("Output: {}", output_device.name().unwrap());

    let buffer_size = 256; // frames per callback (~5.8ms at 44100 Hz)

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(44100),
        buffer_size: cpal::BufferSize::Fixed(buffer_size),
    };

    // Ring buffer: holds 4x the buffer size to absorb timing differences between streams
    let latency_samples = buffer_size as usize * config.channels as usize * 4;
    let rb = HeapRb::<f32>::new(latency_samples);
    let (mut producer, mut consumer) = rb.split();

    // Pre-fill with one buffer worth of silence
    for _ in 0..buffer_size as usize * config.channels as usize {
        producer.try_push(0.0).ok();
    }

    // Effects definition with their settings
    let effects: Arc<Mutex<Vec<EffectSlot>>> = Arc::new(Mutex::new(vec![
        EffectSlot { effect: Box::new(Bitcrusher { bit_depth: 32 }), enabled: true },
        EffectSlot { effect: Box::new(Distortion { drive: 7.0, hard: true }), enabled: true },
        EffectSlot { effect: Box::new(Delay { past_signal: Vec::new(), length: 22050, decay: 0.3}), enabled: true },
    ]));
    let effects_for_closure = Arc::clone(&effects);

    // Input stream: runs whenever new audio samples arrive from the microphone
    // `move` transfers ownership of `producer` into this closure
    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for chunk in data.chunks(2) { // chunk = [left, right]
                    let mono = (chunk[0] + chunk[1]) / 2.0;
                    let mut signal = mono;
                    for effect in effects_for_closure.lock().unwrap().iter_mut() {
                        if effect.enabled {
                            signal = effect.effect.process(signal);
                        }
                    }
                    producer.try_push(signal).ok(); // left output
                    producer.try_push(signal).ok(); // right output
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

    println!("Audio passthrough running. Press Enter to stop.");

    // Loop listening to user input to change effects
    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input == "quit" {
            break; // exit the loop
        } else if input == "e" {
            for effect in effects.lock().unwrap().iter_mut() {
                effect.enabled = !effect.enabled ;
                println!("{}: {}", effect.effect.name(), effect.enabled);
            }
        }
    }
}
