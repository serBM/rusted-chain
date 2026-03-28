use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::{Mutex, Arc};

trait Effect {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32);
    fn name(&self) -> &str;
    fn param_names(&self) -> Vec<&str>;
    fn param_values(&self) -> Vec<String>;
    fn adjust_param(&mut self, index: usize, delta: f32);
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
    fn param_names(&self) -> Vec<&str> {
        vec!["drive", "hard"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.drive.to_string(), self.hard.to_string()]
    }
    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => self.drive = (self.drive + delta * 0.1).max(0.0),
            1 => self.hard = !self.hard,
            _ => {}
        }
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
    fn param_names(&self) -> Vec<&str> {
        vec!["bit_depth"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.bit_depth.to_string()]
    }
    fn adjust_param(&mut self, index: usize, _delta: f32) {
        match index {
            0 => self.bit_depth = (self.bit_depth as i32 + _delta as i32).max(1) as u32,
            _ => {}
        }
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
    fn param_names(&self) -> Vec<&str> {
        vec!["delay_ms", "decay", "ping_pong"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.delay_ms.to_string(), self.decay.to_string(), self.ping_pong.to_string()]
    }
    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => self.delay_ms = (self.delay_ms + delta * 10.0).max(0.0),
            1 => self.decay = (self.decay + delta * 0.05).clamp(0.0, 1.0),
            2 => self.ping_pong = !self.ping_pong,
            _ => {}
        }
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
    fn param_names(&self) -> Vec<&str> {
        vec!["delay_ms", "depth_ms", "lfo_frequency"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.delay_ms.to_string(), self.depth_ms.to_string(), self.lfo_frequency.to_string()]
    }
    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => self.delay_ms = (self.delay_ms + delta * 5.0).max(0.0),
            1 => self.depth_ms = (self.depth_ms + delta * 0.1).max(0.0),
            2 => self.lfo_frequency = (self.lfo_frequency + delta * 0.1).max(0.0),
            _ => {}
        }
    }
}

struct Compressor {
    threshold: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    current_gain: f32,
}

impl Effect for Compressor {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32,f32){
        let target_gain = if left_signal.abs().max(right_signal.abs()) < self.threshold {1.0} else {(self.threshold / left_signal.abs().max(right_signal.abs())).powf(1.0 - 1.0 / self.ratio)};
        let attack_coeff = 1.0 - (-1.0 / (self.attack_ms / 1000.0 * SAMPLE_RATE as f32)).exp();
        let release_coeff = 1.0 - (-1.0 / (self.release_ms / 1000.0 * SAMPLE_RATE as f32)).exp();
        self.current_gain += if target_gain < self.current_gain {(target_gain - self.current_gain) * attack_coeff} else {(target_gain - self.current_gain) * release_coeff};
        (
            left_signal * self.current_gain,
            right_signal * self.current_gain,
        )
    }
    fn name(&self) -> &str {
        "compressor"
    }
    fn param_names(&self) -> Vec<&str> {
        vec!["threshold", "ratio", "attack_ms", "release_ms"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.threshold.to_string(), self.ratio.to_string(), self.attack_ms.to_string(), self.release_ms.to_string()]
    }
    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => self.threshold = (self.threshold + delta * 0.05).clamp(0.0, 1.0),
            1 => self.ratio = (self.ratio + delta).max(1.0),
            2 => self.attack_ms = (self.attack_ms + delta).max(0.1),
            3 => self.release_ms = (self.release_ms + delta).max(0.1),
            _ => {}
        }
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
    fn resize(&mut self) {
        let base_delays_ms = [30.0, 34.0, 39.0, 45.0_f32];
        let sizes = base_delays_ms.map(|ms| (ms * self.room_size * SAMPLE_RATE as f32 / 1000.0) as usize);
        self.left_comb_buffers = [
            vec![0.0_f32; sizes[0]],
            vec![0.0_f32; sizes[1]],
            vec![0.0_f32; sizes[2]],
            vec![0.0_f32; sizes[3]],
        ];
        self.right_comb_buffers = [
            vec![0.0_f32; sizes[0]],
            vec![0.0_f32; sizes[1]],
            vec![0.0_f32; sizes[2]],
            vec![0.0_f32; sizes[3]],
        ];
        self.left_comb_positions = [0; 4];
        self.right_comb_positions = [0; 4];
    }

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
    fn param_names(&self) -> Vec<&str> {
        vec!["room_size", "decay"]
    }
    fn param_values(&self) -> Vec<String> {
        vec![self.room_size.to_string(), self.decay.to_string()]
    }
    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => { self.room_size = (self.room_size + delta * 0.1).max(0.1); self.resize(); }
            1 => self.decay = (self.decay + delta * 0.05).clamp(0.0, 1.0),
            _ => {}
        }
    }
}

struct EffectSlot {
    effect: Box<dyn Effect + Send>,
    enabled: bool,
    wet: f32,
}

// Ratatui
#[derive(PartialEq)]
enum Panel {
    Left,
    Right,
}

struct AppState {
    focused_panel: Panel,
    list_state: ratatui::widgets::ListState,
    selected_effect: usize,
    selected_parameter: usize,
    grabbing: bool,
    show_popup: bool,
    popup_selected: usize,
}

const SAMPLE_RATE: u32 = 44100;
const BUFFER_SIZE: u32 = 256;
const AVAILABLE_EFFECTS: &[&str] = &["distortion", "bitcrusher", "delay", "chorus", "compressor", "reverb"];

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
    let volume: Arc<Mutex<f32>> = Arc::new(Mutex::new(1.0));
    let volume_for_closure = Arc::clone(&volume);

    let effects: Arc<Mutex<Vec<EffectSlot>>> = Arc::new(Mutex::new(vec![
        //EffectSlot { effect: Box::new(Bitcrusher { bit_depth: 32 }), enabled: true },
        //EffectSlot { effect: Box::new(Distortion { drive: 7.0, hard: true }), enabled: true },
        //EffectSlot { effect: Box::new(Delay { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 500.0, decay: 0.3, ping_pong: true}), enabled: true },
        //EffectSlot { effect: Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 40.0, depth_ms: 2.0, lfo_frequency: 1.0, lfo_phase: 0.0}), wet: 1.0, enabled: true },
        //EffectSlot{ effect: Box::new(Compressor { threshold: 0.1, ratio: 20.0, attack_ms: 10.0, release_ms: 200.0, current_gain:1.0}), wet: 1.0, enabled: true },
        //EffectSlot{ effect: Box::new(Reverb::new(10.0,0.7)), wet: 1.0, enabled: true }
        // Shoegaze preset
        EffectSlot { effect: Box::new(Distortion { drive: 1.8, hard: false }), wet: 0.8, enabled: true },
        EffectSlot { effect: Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 30.0, depth_ms: 1.8, lfo_frequency: 0.4, lfo_phase: 0.0}), wet: 1.0, enabled: true },
        EffectSlot { effect: Box::new(Compressor { threshold: 0.7, ratio: 3.0, attack_ms: 10.0, release_ms: 200.0, current_gain: 1.0}), wet: 1.0, enabled: true },
        EffectSlot { effect: Box::new(Reverb::new(2.5, 0.95)), wet: 1.0, enabled: true },
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
                    let vol = *volume_for_closure.lock().unwrap();
                    producer.try_push(left_signal * vol).ok(); // left output
                    producer.try_push(right_signal * vol).ok(); // right output
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
    
    // Ratatui
    let mut state = AppState { focused_panel: Panel::Left, list_state: ratatui::widgets::ListState::default(), selected_effect: 0, selected_parameter: 0, grabbing: false, show_popup: false, popup_selected: 0 };
    state.list_state.select(Some(0));

    crossterm::terminal::enable_raw_mode().unwrap();
    crossterm::execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All)).unwrap();
    let mut terminal = ratatui::Terminal::new(
      ratatui::backend::CrosstermBackend::new(std::io::stdout())
    ).unwrap();

    loop {
        terminal.draw(|frame| {
            let effects_lock = effects.lock().unwrap();
            let items: Vec<ratatui::widgets::ListItem> = effects_lock
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let item = ratatui::widgets::ListItem::new(e.effect.name());
                if state.grabbing && i == state.selected_effect {
                    item.style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow))
                } else {
                    item
                }
            })
            .collect();
            let highlight_style = ratatui::style::Style::default().fg(ratatui::style::Color::Yellow);
            let normal_style = ratatui::style::Style::default();
            let param_items: Vec<ratatui::widgets::ListItem> = if effects_lock.is_empty() {
                vec![]
            } else {
                let selected_slot = &effects_lock[state.selected_effect];
                let wet_item = ratatui::widgets::ListItem::new(format!("wet: {}", selected_slot.wet))
                    .style(if state.focused_panel == Panel::Right && state.selected_parameter == 0 { highlight_style } else { normal_style });
                let mut items = vec![wet_item];
                let names = selected_slot.effect.param_names();
                let values = selected_slot.effect.param_values();
                items.extend(
                    names.iter().zip(values.iter()).enumerate()
                        .map(|(i, (name, value))| {
                            ratatui::widgets::ListItem::new(format!("{}: {}", name, value))
                                .style(if state.focused_panel == Panel::Right && state.selected_parameter == i + 1 { highlight_style } else { normal_style })
                        })
                );
                items
            };
            let vertical = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(3),
                    ratatui::layout::Constraint::Min(0),
                    ratatui::layout::Constraint::Length(3),
                ])
                .split(frame.area());
            let key_style = ratatui::style::Style::default().fg(ratatui::style::Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD);
            let desc_style = ratatui::style::Style::default();
            let keybind_pairs: &[(&str, &str)] = if state.show_popup {
                &[("↑↓", "navigate"), ("Enter", "confirm"), ("Esc", "cancel")]
            } else if state.focused_panel == Panel::Left {
                &[("↑↓", "navigate"), ("Enter", "grab/release"), ("Del", "delete"), ("A", "add"), ("Tab", "switch panel"), ("Q", "quit")]
            } else {
                &[("↑↓", "navigate"), ("←→", "change value"), ("Tab", "switch panel"), ("Q", "quit")]
            };
            let keybind_spans: Vec<ratatui::text::Span> = keybind_pairs.iter().flat_map(|(key, desc)| {
                vec![
                    ratatui::text::Span::styled(format!(" {} ", key), key_style),
                    ratatui::text::Span::styled(format!("{} ", desc), desc_style),
                ]
            }).collect();
            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::from(keybind_spans))
                    .block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL)),
                vertical[0],
            );
            let areas = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([
                    ratatui::layout::Constraint::Percentage(30),
                    ratatui::layout::Constraint::Percentage(70),
                ])
                .split(vertical[1]);
            let vol = *volume.lock().unwrap();
            frame.render_widget(
                ratatui::widgets::Paragraph::new(format!("Volume: {:.2}", vol))
                    .block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL)),
                vertical[2],
            );
            let left_border_style = if state.focused_panel == Panel::Left { highlight_style } else { normal_style };
            let right_border_style = if state.focused_panel == Panel::Right { highlight_style } else { normal_style };
            frame.render_widget(
                ratatui::widgets::List::new(param_items)
                    .block(ratatui::widgets::Block::default().title("Parameters").borders(ratatui::widgets::Borders::ALL).border_style(right_border_style)),
                areas[1],
            );
            frame.render_stateful_widget(
        ratatui::widgets::List::new(items)
                    .block(ratatui::widgets::Block::default().title("Effects").borders(ratatui::widgets::Borders::ALL).border_style(left_border_style))
                    .highlight_symbol(">> "),
                areas[0],
                &mut state.list_state,
            );
            if state.show_popup {
                let popup_items: Vec<ratatui::widgets::ListItem> = AVAILABLE_EFFECTS.iter().enumerate()
                    .map(|(i, name)| {
                        ratatui::widgets::ListItem::new(*name)
                            .style(if i == state.popup_selected { highlight_style } else { normal_style })
                    })
                    .collect();
                let popup_area = ratatui::layout::Rect {
                    x: frame.area().width / 4,
                    y: frame.area().height / 4,
                    width: frame.area().width / 2,
                    height: AVAILABLE_EFFECTS.len() as u16 + 2,
                };
                frame.render_widget(ratatui::widgets::Clear, popup_area);
                frame.render_widget(
                    ratatui::widgets::List::new(popup_items)
                        .block(ratatui::widgets::Block::default().title("Add Effect").borders(ratatui::widgets::Borders::ALL)),
                    popup_area,
                );
            }
        }).unwrap();
                                                                                                                                                                                                                                                                                            
        if crossterm::event::poll(std::time::Duration::from_millis(16)).unwrap() {
            if let crossterm::event::Event::Key(key) = crossterm::event::read().unwrap() {
                if key.code == crossterm::event::KeyCode::Char('q') || (key.code == crossterm::event::KeyCode::Char('c') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)) {
                    crossterm::terminal::disable_raw_mode().unwrap();
                    break;
                }
                if !state.show_popup {
                if key.code == crossterm::event::KeyCode::Enter && state.focused_panel == Panel::Left {
                    state.grabbing = !state.grabbing;
                }
                if key.code == crossterm::event::KeyCode::Up && !effects.lock().unwrap().is_empty() {
                    if state.focused_panel == Panel::Left {
                        if state.grabbing {
                            let mut effects_lock = effects.lock().unwrap();
                            if state.selected_effect > 0 {
                                effects_lock.swap(state.selected_effect, state.selected_effect - 1);
                                state.selected_effect -= 1;
                                state.list_state.select(Some(state.selected_effect));
                            }
                        } else {
                            if state.selected_effect > 0 { state.selected_effect -= 1; } else { state.selected_effect = effects.lock().unwrap().len() - 1; }
                            state.list_state.select(Some(state.selected_effect));
                        }
                    } else if state.focused_panel == Panel::Right {
                        let param_count = 1 + effects.lock().unwrap()[state.selected_effect].effect.param_names().len();
                        if state.selected_parameter > 0 { state.selected_parameter -= 1; } else { state.selected_parameter = param_count - 1; }
                    }
                }
                if key.code == crossterm::event::KeyCode::Down && !effects.lock().unwrap().is_empty() {
                    if state.focused_panel == Panel::Left {
                        if state.grabbing {
                            let mut effects_lock = effects.lock().unwrap();
                            if state.selected_effect < effects_lock.len() - 1 {
                                effects_lock.swap(state.selected_effect, state.selected_effect + 1);
                                state.selected_effect += 1;
                                state.list_state.select(Some(state.selected_effect));
                            }
                        } else {
                            if state.selected_effect < effects.lock().unwrap().len() - 1 { state.selected_effect += 1; } else { state.selected_effect = 0; }
                            state.list_state.select(Some(state.selected_effect));
                        }
                    } else if state.focused_panel == Panel::Right {
                        let param_count = 1 + effects.lock().unwrap()[state.selected_effect].effect.param_names().len();
                        if state.selected_parameter < param_count - 1 { state.selected_parameter += 1; } else { state.selected_parameter = 0; }
                    }
                }
                if key.code == crossterm::event::KeyCode::Left && state.focused_panel == Panel::Right && !effects.lock().unwrap().is_empty() {
                    let mut effects_lock = effects.lock().unwrap();
                    let slot = &mut effects_lock[state.selected_effect];
                    if state.selected_parameter == 0 {
                        slot.wet = (slot.wet - 0.05).clamp(0.0, 1.0);
                    } else {
                        slot.effect.adjust_param(state.selected_parameter - 1, -1.0);
                    }
                }
                if key.code == crossterm::event::KeyCode::Right && state.focused_panel == Panel::Right && !effects.lock().unwrap().is_empty() {
                    let mut effects_lock = effects.lock().unwrap();
                    let slot = &mut effects_lock[state.selected_effect];
                    if state.selected_parameter == 0 {
                        slot.wet = (slot.wet + 0.05).clamp(0.0, 1.0);
                    } else {
                        slot.effect.adjust_param(state.selected_parameter - 1, 1.0);
                    }
                }
                if key.code == crossterm::event::KeyCode::PageUp {
                    let mut vol = volume.lock().unwrap();
                    *vol = (*vol + 0.05).min(2.0);
                }
                if key.code == crossterm::event::KeyCode::PageDown {
                    let mut vol = volume.lock().unwrap();
                    *vol = (*vol - 0.05).max(0.0);
                }
                if key.code == crossterm::event::KeyCode::Delete && state.focused_panel == Panel::Left {
                    let mut effects_lock = effects.lock().unwrap();
                    if !effects_lock.is_empty() {
                        effects_lock.remove(state.selected_effect);
                        if state.selected_effect >= effects_lock.len() && state.selected_effect > 0 {
                            state.selected_effect -= 1;
                        }
                        state.list_state.select(Some(state.selected_effect));
                    }
                }
                } // end !show_popup
                if key.code == crossterm::event::KeyCode::Char('a') && state.focused_panel == Panel::Left && !state.show_popup {
                    state.show_popup = true;
                    state.popup_selected = 0;
                }
                if state.show_popup {
                    if key.code == crossterm::event::KeyCode::Esc {
                        state.show_popup = false;
                    }
                    if key.code == crossterm::event::KeyCode::Up {
                        if state.popup_selected > 0 { state.popup_selected -= 1; } else { state.popup_selected = AVAILABLE_EFFECTS.len() - 1; }
                    }
                    if key.code == crossterm::event::KeyCode::Down {
                        if state.popup_selected < AVAILABLE_EFFECTS.len() - 1 { state.popup_selected += 1; } else { state.popup_selected = 0; }
                    }
                    if key.code == crossterm::event::KeyCode::Enter {
                        let new_effect: Box<dyn Effect + Send> = match state.popup_selected {
                            0 => Box::new(Distortion { drive: 2.0, hard: false }),
                            1 => Box::new(Bitcrusher { bit_depth: 8 }),
                            2 => Box::new(Delay { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 300.0, decay: 0.4, ping_pong: false }),
                            3 => Box::new(Chorus { past_left_signal: Vec::new(), past_right_signal: Vec::new(), delay_ms: 30.0, depth_ms: 2.0, lfo_frequency: 1.0, lfo_phase: 0.0 }),
                            4 => Box::new(Compressor { threshold: 0.7, ratio: 4.0, attack_ms: 10.0, release_ms: 200.0, current_gain: 1.0 }),
                            _ => Box::new(Reverb::new(1.0, 0.5)),
                        };
                        effects.lock().unwrap().push(EffectSlot { effect: new_effect, enabled: true, wet: 1.0 });
                        state.show_popup = false;
                    }
                }
                if key.code == crossterm::event::KeyCode::Tab && !state.show_popup {
                    state.focused_panel = if state.focused_panel == Panel::Left {Panel::Right} else {Panel::Left};
                }
            }
        }                                                                                                                                                                                                                                                                                   
    }
}
