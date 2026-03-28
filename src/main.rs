use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::{Mutex, Arc};

trait Effect {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32);
    fn name(&self) -> &str;
}

struct Distortion {
    drive: f32,
    hard: bool,
}

impl Effect for Distortion {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32) {
        if self.hard {
            (
                (left_signal * self.drive).clamp(-1.0, 1.0),
                (right_signal * self.drive).clamp(-1.0, 1.0),
            )
        } else {
            (
                (left_signal * self.drive).tanh(),
                (right_signal * self.drive).tanh(),
            )
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
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        let steps = 2_f32.powi(self.bit_depth as i32);
        (
            (left_signal * steps as f32).round() / steps as f32,
            (right_signal * steps as f32).round() / steps as f32,
        )
    }
    fn name(&self) -> &str {
        "bitcrusher"
    }
}

struct Delay {
    past_left_signal: Vec<f32>,
    past_right_signal: Vec<f32>,
    delay_ms: f32,
    decay: f32,
    ping_pong: bool,
}

impl Effect for Delay {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        // if delay_ms is 0.0 we clear the buffers and send dry signal directly
        if self.delay_ms==0.0 {
            self.past_left_signal.clear();
            self.past_right_signal.clear();
            return (left_signal,right_signal)
        }
        // get delayed signal or 0.0 if past_signal is still empty
        let delayed_left = self.past_left_signal.get(0).copied().unwrap_or(0.0);
        let delayed_right = self.past_right_signal.get(0).copied().unwrap_or(0.0);
        // mix signal + delay
        let left_output: f32 = left_signal + (delayed_left * self.decay);
        let right_output: f32 = right_signal + (delayed_right * self.decay);
        // push current signal to the buffer
        if self.ping_pong{
            self.past_left_signal.push(right_output);
            self.past_right_signal.push(left_output);
        } else {
            self.past_left_signal.push(left_output);
            self.past_right_signal.push(right_output);
        }
        // clean past_signal buffer
        let length = (SAMPLE_RATE as f32 / 1000.0 * self.delay_ms) as usize;
        if self.past_left_signal.len() > length {
            self.past_left_signal.remove(0);
        }
        if self.past_right_signal.len() > length {
            self.past_right_signal.remove(0);
        }
        (
            left_output,
            right_output,
        )
    }
    fn name(&self) -> &str {
        "delay"
    }
}

struct Chorus {
    delay_ms: f32,
    depth_ms: f32,
    lfo_frequency: f32,
    lfo_phase: f32,
    past_left_signal: Vec<f32>,
    past_right_signal: Vec<f32>,
}

impl Effect for Chorus {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        // if delay_ms is 0.0 we clear the buffers and send dry signal directly
        if self.delay_ms==0.0 {
            self.past_left_signal.clear();
            self.past_right_signal.clear();
            return (left_signal,right_signal)
        }
        // get delayed signal or 0.0 if past_signal is still empty
        let delayed_left = self.past_left_signal.get(0).copied().unwrap_or(0.0);
        let delayed_right = self.past_right_signal.get(0).copied().unwrap_or(0.0);
        // mix signal + delay
        let left_output: f32 = left_signal + delayed_left;
        let right_output: f32 = right_signal + delayed_right;
        // push current signal to the buffer
        self.past_left_signal.push(left_signal);
        self.past_right_signal.push(right_signal);
        // clean past_signal buffer
        let left_current_delay_ms = self.delay_ms + (self.depth_ms * self.lfo_phase.sin());
        let left_length = (SAMPLE_RATE as f32 / 1000.0 * left_current_delay_ms) as usize;
        let right_current_delay_ms = self.delay_ms + (self.depth_ms * (self.lfo_phase + std::f32::consts::PI / 2.0).sin());
        let right_length = (SAMPLE_RATE as f32 / 1000.0 * right_current_delay_ms) as usize;
        while self.past_left_signal.len() > left_length {
            self.past_left_signal.remove(0);
        }
        while self.past_right_signal.len() > right_length {
            self.past_right_signal.remove(0);
        }
        // advance the phase
        self.lfo_phase += 2.0 * std::f32::consts::PI * self.lfo_frequency / SAMPLE_RATE as f32;
        self.lfo_phase = self.lfo_phase%(2.0*std::f32::consts::PI);
        (
            left_output,
            right_output,
        )
    }
    fn name(&self) -> &str {
        "chorus"
    }
}

struct Compressor {
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    current_gain: f32,
}

impl Effect for Compressor {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        let target_gain = if left_signal.abs().max(right_signal.abs()) < self.threshold {1.0} else {(self.threshold / left_signal.abs().max(right_signal.abs())).powf(1.0 - 1.0 / self.ratio)};
        self.current_gain += if target_gain < self.current_gain {(target_gain - self.current_gain) * self.attack} else {(target_gain - self.current_gain) * self.release};
        (
            left_signal * self.current_gain,
            right_signal * self.current_gain,
        )
    }
    fn name(&self) -> &str {
        "compressor"
    }
}

struct Reverb {
    room_size: f32,
    decay: f32,
    left_comb_buffers: [Vec<f32>; 4],
    right_comb_buffers: [Vec<f32>; 4],
    left_comb_positions: [usize; 4],
    right_comb_positions: [usize; 4],
}

impl Reverb {
    fn new(room_size: f32, decay: f32) -> Self {
        // compute buffer size, initialize buffers
        let base_delays_ms = [30.0, 34.0, 39.0, 45.0_f32];
        let size0 = (base_delays_ms[0] * room_size * SAMPLE_RATE as f32 / 1000.0) as usize;
        let size1 = (base_delays_ms[1] * room_size * SAMPLE_RATE as f32 / 1000.0) as usize;
        let size2 = (base_delays_ms[2] * room_size * SAMPLE_RATE as f32 / 1000.0) as usize;
        let size3 = (base_delays_ms[3] * room_size * SAMPLE_RATE as f32 / 1000.0) as usize;
        Self {
            room_size: room_size,
            decay: decay,
            left_comb_buffers: [
                vec![0.0_f32; size0],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size1],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size2],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size3],
            ],
            right_comb_buffers: [
                vec![0.0_f32; size0],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size1],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size2],                                                                                                                                                                                                                                                               
                vec![0.0_f32; size3],
            ],
            left_comb_positions: [0 ; 4],
            right_comb_positions: [0 ; 4],
        }
    }
}

impl Effect for Reverb {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        let mut left_sum = 0.0_f32;
        let mut right_sum = 0.0_f32;
        for i in 0..4 {
            // left reverb
            let left_delayed = self.left_comb_buffers[i][self.left_comb_positions[i]];
            left_sum += left_delayed;
            self.left_comb_buffers[i][self.left_comb_positions[i]] = left_signal + left_delayed * self.decay;
            self.left_comb_positions[i] = (self.left_comb_positions[i] + 1) % self.left_comb_buffers[i].len();
            // right reverb
            let right_delayed = self.right_comb_buffers[i][self.right_comb_positions[i]];
            right_sum += right_delayed;
            self.right_comb_buffers[i][self.right_comb_positions[i]] = right_signal + right_delayed * self.decay;
            self.right_comb_positions[i] = (self.right_comb_positions[i] + 1) % self.right_comb_buffers[i].len();
        }
        let left_reverbed_signal = left_sum / 4.0;
        let right_reverbed_signal = right_sum / 4.0;
        (
            left_signal + left_reverbed_signal,
            right_signal + right_reverbed_signal,
        )
    }
    fn name(&self) -> &str {
        "reverb"
    }
}

struct EffectSlot {
    effect: Box<dyn Effect + Send>,
    enabled: bool,
    wet: f32,
}

const SAMPLE_RATE: u32 = 44100;
const BUFFER_SIZE: u32 = 256;

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

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Fixed(BUFFER_SIZE),
    };

    // Ring buffer: holds 4x the buffer size to absorb timing differences between streams
    let latency_samples = BUFFER_SIZE as usize * config.channels as usize * 4;
    let rb = HeapRb::<f32>::new(latency_samples);
    let (mut producer, mut consumer) = rb.split();

    // Pre-fill with one buffer worth of silence
    for _ in 0..BUFFER_SIZE as usize * config.channels as usize {
        producer.try_push(0.0).ok();
    }

    // Effects definition with their settings
    let effects: Arc<Mutex<Vec<EffectSlot>>> = Arc::new(Mutex::new(vec![
        //EffectSlot { effect: Box::new(Bitcrusher { bit_depth: 32 }), enabled: true },
        //EffectSlot { effect: Box::new(Distortion { drive: 7.0, hard: true }), enabled: true },
        //EffectSlot { effect: Box::new(Delay { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 500.0, decay: 0.3, ping_pong: true}), enabled: true },
        //EffectSlot { effect: Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 40.0, depth_ms: 2.0, lfo_frequency: 1.0, lfo_phase: 0.0}), wet: 1.0, enabled: true },
        //EffectSlot{ effect: Box::new(Compressor { threshold: 0.1, ratio: 20.0, attack: 0.5, release: 0.001, current_gain:1.0}), wet: 1.0, enabled: true },
        //EffectSlot{ effect: Box::new(Reverb::new(10.0,0.7)), wet: 1.0, enabled: true }
        // Shoegaze preset
        EffectSlot { effect: Box::new(Distortion { drive: 1.8, hard: false }), wet: 0.8, enabled: true },
        EffectSlot { effect: Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 30.0, depth_ms: 1.8, lfo_frequency: 0.4, lfo_phase: 0.0}), wet: 1.0, enabled: true },
        EffectSlot { effect: Box::new(Compressor { threshold: 0.7, ratio: 3.0, attack: 0.05, release: 0.005, current_gain: 1.0}), wet: 1.0, enabled: true },
        EffectSlot { effect: Box::new(Reverb::new(2.5, 0.7)), wet: 1.0, enabled: true },
    ]));
    let effects_for_closure = Arc::clone(&effects);

    // Input stream: runs whenever new audio samples arrive from the microphone
    // `move` transfers ownership of `producer` into this closure
    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for chunk in data.chunks(2) { // chunk = [left, right]
                    let mut left_signal = chunk[0];
                    let mut right_signal = chunk[0];
                    for effect in effects_for_closure.lock().unwrap().iter_mut() {
                        if effect.enabled {
                            let (processed_left, processed_right) = effect.effect.process(left_signal, right_signal);
                            left_signal = (1.0 - effect.wet) * left_signal+ effect.wet * processed_left;
                            right_signal = (1.0 - effect.wet) * right_signal + effect.wet * processed_right;
                        }
                    }
                    producer.try_push(left_signal).ok(); // left output
                    producer.try_push(right_signal).ok(); // right output
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
