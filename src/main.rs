use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::{Mutex, Arc};

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

    // Value to enable or disable distortion effect
    let distortion_enabled: Arc<Mutex<bool>> = Arc::new(Mutex::new(true));
    let distortion_enabled_for_closure = Arc::clone(&distortion_enabled);

    // Drive setting for distortion effect (we multiply the signal by this value
    let drive: f32 = 5.0;

    // Input stream: runs whenever new audio samples arrive from the microphone
    // `move` transfers ownership of `producer` into this closure
    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for chunk in data.chunks(2) { // chunk = [left, right]
                    let mono = (chunk[0] + chunk[1]) / 2.0;
                    let mut signal = mono;
                    let distortion_enabled = distortion_enabled_for_closure.lock().unwrap();
                    if *distortion_enabled {
                        signal = distortion_soft(mono, drive); // apply distortion effect
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
        } else if input == "d" {
            let mut distortion_value = distortion_enabled.lock().unwrap();
            *distortion_value = !*distortion_value;
            println!("Distortion active : {distortion_value}")
        }
    }
}

#[allow(dead_code)]
fn distortion_hard(signal: f32, drive: f32) -> f32 {
    (signal * drive).clamp(-1.0, 1.0)
}

fn distortion_soft(signal: f32, drive: f32) -> f32 {
    (signal * drive).tanh()
}
