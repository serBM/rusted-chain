use crate::preset::PresetEffect;

pub const SAMPLE_RATE: u32 = 48000;
pub const BUFFER_SIZE: u32 = 128;
pub const AVAILABLE_EFFECTS: &[&str] = &["distortion", "bitcrusher", "delay", "chorus", "compressor", "reverb", "tremolo"];

pub trait Effect {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32);
    fn name(&self) -> &str;
    fn param_names(&self) -> Vec<&str>;
    fn param_values(&self) -> Vec<String>;
    fn adjust_param(&mut self, index: usize, delta: f32);
    fn to_preset(&self) -> PresetEffect;
}

pub struct EffectSlot {
    pub effect: Box<dyn Effect + Send>,
    pub enabled: bool,
    pub wet: f32,
}

pub struct Distortion {
    pub drive: f32,
    pub hard: bool,
}

impl Effect for Distortion {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Distortion { drive: self.drive, hard: self.hard }
    }
}

pub struct Bitcrusher {
    pub bit_depth: u32,
}

impl Effect for Bitcrusher {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Bitcrusher { bit_depth: self.bit_depth }
    }
}

pub struct Delay {
    pub past_left_signal: Vec<f32>,
    pub past_right_signal: Vec<f32>,
    pub delay_ms: f32,
    pub decay: f32,
    pub ping_pong: bool,
}

impl Effect for Delay {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
        // if delay_ms is 0.0 we clear the buffers and send dry signal directly
        if self.delay_ms == 0.0 {
            self.past_left_signal.clear();
            self.past_right_signal.clear();
            return (left_signal, right_signal);
        }
        // get delayed signal or 0.0 if past_signal is still empty
        let delayed_left = self.past_left_signal.get(0).copied().unwrap_or(0.0);
        let delayed_right = self.past_right_signal.get(0).copied().unwrap_or(0.0);
        // mix signal + delay
        let left_output: f32 = left_signal + (delayed_left * self.decay);
        let right_output: f32 = right_signal + (delayed_right * self.decay);
        // push current signal to the buffer
        if self.ping_pong {
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
        (left_output, right_output)
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Delay { delay_ms: self.delay_ms, decay: self.decay, ping_pong: self.ping_pong }
    }
}

pub struct Chorus {
    pub delay_ms: f32,
    pub depth_ms: f32,
    pub lfo_frequency: f32,
    pub lfo_phase: f32,
    pub past_left_signal: Vec<f32>,
    pub past_right_signal: Vec<f32>,
}

impl Effect for Chorus {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
        // if delay_ms is 0.0 we clear the buffers and send dry signal directly
        if self.delay_ms == 0.0 {
            self.past_left_signal.clear();
            self.past_right_signal.clear();
            return (left_signal, right_signal);
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
        self.lfo_phase = self.lfo_phase % (2.0 * std::f32::consts::PI);
        (left_output, right_output)
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Chorus { delay_ms: self.delay_ms, depth_ms: self.depth_ms, lfo_frequency: self.lfo_frequency }
    }
}

pub struct Compressor {
    pub threshold: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub current_gain: f32,
}

impl Effect for Compressor {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
        let target_gain = if left_signal.abs().max(right_signal.abs()) < self.threshold { 1.0 } else { (self.threshold / left_signal.abs().max(right_signal.abs())).powf(1.0 - 1.0 / self.ratio) };
        let attack_coeff = 1.0 - (-1.0 / (self.attack_ms / 1000.0 * SAMPLE_RATE as f32)).exp();
        let release_coeff = 1.0 - (-1.0 / (self.release_ms / 1000.0 * SAMPLE_RATE as f32)).exp();
        self.current_gain += if target_gain < self.current_gain { (target_gain - self.current_gain) * attack_coeff } else { (target_gain - self.current_gain) * release_coeff };
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Compressor { threshold: self.threshold, ratio: self.ratio, attack_ms: self.attack_ms, release_ms: self.release_ms }
    }
}

pub struct Reverb {
    pub room_size: f32,
    pub decay: f32,
    pub left_comb_buffers: [Vec<f32>; 4],
    pub right_comb_buffers: [Vec<f32>; 4],
    pub left_comb_positions: [usize; 4],
    pub right_comb_positions: [usize; 4],
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

    pub fn new(room_size: f32, decay: f32) -> Self {
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
            left_comb_positions: [0; 4],
            right_comb_positions: [0; 4],
        }
    }
}

impl Effect for Reverb {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
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
    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Reverb { room_size: self.room_size, decay: self.decay }
    }
}
pub struct Tremolo {
    pub depth: f32,
    pub lfo_frequency: f32,
    pub lfo_phase: f32,
}

impl Effect for Tremolo {
    fn process(&mut self, left_signal: f32, right_signal: f32) -> (f32, f32) {
        // mix signal + delay
        let left_output = left_signal * (1.0 - self.depth * (1.0 - self.lfo_phase.sin()) / 2.0 );
        let right_output = right_signal * (1.0 - self.depth * (1.0 - self.lfo_phase.sin()) / 2.0 );
        // advance the phase
        self.lfo_phase += 2.0 * std::f32::consts::PI * self.lfo_frequency / SAMPLE_RATE as f32;
        self.lfo_phase = self.lfo_phase % (2.0 * std::f32::consts::PI);
        (
            left_output, 
            right_output
        )
    }

    fn name(&self) -> &str {
        "tremolo"
    }

    fn param_names(&self) -> Vec<&str> {
        vec!["depth", "lfo_frequency"]
    }

    fn param_values(&self) -> Vec<String> {
        vec![self.depth.to_string(), self.lfo_frequency.to_string()]
    }

    fn adjust_param(&mut self, index: usize, delta: f32) {
        match index {
            0 => self.depth = (self.depth + delta * 0.05).clamp(0.0, 1.0),
            1 => self.lfo_frequency = (self.lfo_frequency + delta * 0.1).max(0.0),
            _ => {}
        }
    }

    fn to_preset(&self) -> PresetEffect {
        PresetEffect::Tremolo {depth: self.depth, lfo_frequency: self.lfo_frequency}
    }
}