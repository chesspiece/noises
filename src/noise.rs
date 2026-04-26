use core::fmt;

/// Supported noise colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum NoiseKind {
    /// Flat power across the spectrum.
    White = 0,
    /// 1/f power distribution, usually perceived as smoother than white noise.
    Pink = 1,
    /// 1/f² power distribution, also called Brownian or red noise.
    Brown = 2,
}

/// Generator configuration shared by native Rust callers and the C ABI.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct NoiseConfig {
    /// Noise color to generate.
    pub kind: NoiseKind,
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved output channels.
    pub channels: u16,
    /// Linear output gain in the inclusive range `[0.0, 1.0]`.
    pub amplitude: f32,
    /// Seed for the internal pseudo-random number generator.
    pub seed: u64,
}

impl NoiseConfig {
    /// Creates a configuration with sane defaults for real-time playback.
    pub const fn new(kind: NoiseKind) -> Self {
        Self {
            kind,
            sample_rate: 48_000,
            channels: 2,
            amplitude: 0.2,
            seed: 0x4D59_1DF4_D0F3_3173,
        }
    }

    /// Validates the configuration before constructing a generator.
    pub fn validate(self) -> Result<Self, NoiseError> {
        if self.sample_rate == 0 {
            return Err(NoiseError::SampleRateZero);
        }
        if self.channels == 0 {
            return Err(NoiseError::ChannelsZero);
        }
        if !(0.0..=1.0).contains(&self.amplitude) {
            return Err(NoiseError::AmplitudeOutOfRange);
        }
        Ok(self)
    }
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self::new(NoiseKind::Pink)
    }
}

/// Errors returned when a generator configuration is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseError {
    /// `sample_rate` must be greater than zero.
    SampleRateZero,
    /// `channels` must be greater than zero.
    ChannelsZero,
    /// `amplitude` must stay within the inclusive `[0.0, 1.0]` range.
    AmplitudeOutOfRange,
}

impl fmt::Display for NoiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleRateZero => f.write_str("sample rate must be greater than zero"),
            Self::ChannelsZero => f.write_str("channel count must be greater than zero"),
            Self::AmplitudeOutOfRange => f.write_str("amplitude must be between 0.0 and 1.0"),
        }
    }
}

impl std::error::Error for NoiseError {}

/// Stateful real-time generator that produces a continuous, non-looping stream.
#[derive(Debug, Clone)]
pub struct NoiseGenerator {
    config: NoiseConfig,
    rng: SplitMix64,
    pink: PinkState,
    brown: BrownState,
}

impl NoiseGenerator {
    /// Creates a new generator from a validated configuration.
    pub fn new(config: NoiseConfig) -> Result<Self, NoiseError> {
        let config = config.validate()?;
        Ok(Self {
            rng: SplitMix64::new(config.seed),
            pink: PinkState::default(),
            brown: BrownState::new(config.sample_rate),
            config,
        })
    }

    /// Returns the current configuration snapshot.
    pub const fn config(&self) -> NoiseConfig {
        self.config
    }

    /// Returns the active noise color.
    pub const fn kind(&self) -> NoiseKind {
        self.config.kind
    }

    /// Changes the generated noise color without rebuilding the generator.
    pub fn set_kind(&mut self, kind: NoiseKind) {
        self.config.kind = kind;
    }

    /// Returns the configured channel count.
    pub const fn channels(&self) -> u16 {
        self.config.channels
    }

    /// Updates the channel count used by [`fill_interleaved`](Self::fill_interleaved).
    pub fn set_channels(&mut self, channels: u16) -> Result<(), NoiseError> {
        if channels == 0 {
            return Err(NoiseError::ChannelsZero);
        }
        self.config.channels = channels;
        Ok(())
    }

    /// Returns the configured sample rate.
    pub const fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// Updates the sample rate and re-tunes the brown-noise integrator.
    pub fn set_sample_rate(&mut self, sample_rate: u32) -> Result<(), NoiseError> {
        if sample_rate == 0 {
            return Err(NoiseError::SampleRateZero);
        }
        self.config.sample_rate = sample_rate;
        self.brown.set_sample_rate(sample_rate);
        Ok(())
    }

    /// Returns the current linear gain.
    pub const fn amplitude(&self) -> f32 {
        self.config.amplitude
    }

    /// Sets the output gain.
    pub fn set_amplitude(&mut self, amplitude: f32) -> Result<(), NoiseError> {
        if !(0.0..=1.0).contains(&amplitude) {
            return Err(NoiseError::AmplitudeOutOfRange);
        }
        self.config.amplitude = amplitude;
        Ok(())
    }

    /// Resets the pseudo-random stream and all filter state to a known seed.
    pub fn reseed(&mut self, seed: u64) {
        self.config.seed = seed;
        self.rng = SplitMix64::new(seed);
        self.pink.reset();
        self.brown.reset();
    }

    /// Generates one mono sample in the inclusive range `[-1.0, 1.0]`.
    pub fn next_mono(&mut self) -> f32 {
        let white = self.rng.next_signed_f32();
        let colored = match self.config.kind {
            NoiseKind::White => white,
            NoiseKind::Pink => self.pink.next(white),
            NoiseKind::Brown => self.brown.next(white),
        };
        (colored * self.config.amplitude).clamp(-1.0, 1.0)
    }

    /// Fills a mono buffer with newly generated samples.
    pub fn fill_mono(&mut self, output: &mut [f32]) {
        for sample in output {
            *sample = self.next_mono();
        }
    }

    /// Fills an interleaved buffer, duplicating each generated sample into every channel.
    pub fn fill_interleaved(&mut self, output: &mut [f32]) {
        let channels = usize::from(self.config.channels);
        for frame in output.chunks_mut(channels) {
            let sample = self.next_mono();
            frame.fill(sample);
        }
    }
}

#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_signed_f32(&mut self) -> f32 {
        let unit = ((self.next_u64() >> 40) as f32) / ((1u32 << 24) as f32);
        unit.mul_add(2.0, -1.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PinkState {
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
}

impl PinkState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn next(&mut self, white: f32) -> f32 {
        self.b0 = 0.998_86 * self.b0 + white * 0.055_517_9;
        self.b1 = 0.993_32 * self.b1 + white * 0.075_075_9;
        self.b2 = 0.969_00 * self.b2 + white * 0.153_852_0;
        self.b3 = 0.866_50 * self.b3 + white * 0.310_485_6;
        self.b4 = 0.550_00 * self.b4 + white * 0.532_952_2;
        self.b5 = -0.761_6 * self.b5 - white * 0.016_898_0;

        let pink =
            self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.5362;
        self.b6 = white * 0.115_926;

        (pink * 0.11).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy)]
struct BrownState {
    value: f32,
    step: f32,
}

impl BrownState {
    fn new(sample_rate: u32) -> Self {
        let mut state = Self {
            value: 0.0,
            step: 0.0,
        };
        state.set_sample_rate(sample_rate);
        state
    }

    fn reset(&mut self) {
        self.value = 0.0;
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        let sample_rate = sample_rate.max(1) as f32;
        self.step = 0.02 * (48_000.0 / sample_rate).sqrt();
    }

    fn next(&mut self, white: f32) -> f32 {
        self.value = (self.value + white * self.step) / 1.02;
        (self.value * 3.5).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{NoiseConfig, NoiseError, NoiseGenerator, NoiseKind};

    #[test]
    fn invalid_configs_are_rejected() {
        assert_eq!(
            NoiseGenerator::new(NoiseConfig {
                sample_rate: 0,
                ..NoiseConfig::default()
            })
            .unwrap_err(),
            NoiseError::SampleRateZero
        );
        assert_eq!(
            NoiseGenerator::new(NoiseConfig {
                channels: 0,
                ..NoiseConfig::default()
            })
            .unwrap_err(),
            NoiseError::ChannelsZero
        );
        assert_eq!(
            NoiseGenerator::new(NoiseConfig {
                amplitude: 1.2,
                ..NoiseConfig::default()
            })
            .unwrap_err(),
            NoiseError::AmplitudeOutOfRange
        );
    }

    #[test]
    fn same_seed_reproduces_the_same_stream() {
        let config = NoiseConfig {
            kind: NoiseKind::Pink,
            seed: 42,
            ..NoiseConfig::default()
        };

        let mut first = NoiseGenerator::new(config).unwrap();
        let mut second = NoiseGenerator::new(config).unwrap();
        let mut first_buffer = [0.0f32; 256];
        let mut second_buffer = [0.0f32; 256];

        first.fill_mono(&mut first_buffer);
        second.fill_mono(&mut second_buffer);

        assert_eq!(first_buffer, second_buffer);
    }

    #[test]
    fn interleaved_fill_duplicates_each_generated_frame() {
        let mut generator = NoiseGenerator::new(NoiseConfig {
            kind: NoiseKind::Brown,
            channels: 2,
            seed: 7,
            ..NoiseConfig::default()
        })
        .unwrap();

        let mut buffer = [0.0f32; 16];
        generator.fill_interleaved(&mut buffer);

        for frame in buffer.chunks_exact(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn all_kinds_stay_finite_and_bounded() {
        for kind in [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown] {
            let mut generator = NoiseGenerator::new(NoiseConfig {
                kind,
                seed: 123,
                ..NoiseConfig::default()
            })
            .unwrap();

            let mut buffer = [0.0f32; 8192];
            generator.fill_mono(&mut buffer);

            assert!(buffer.iter().all(|sample| sample.is_finite()));
            assert!(buffer.iter().all(|sample| sample.abs() <= 1.0));
        }
    }

    #[test]
    fn reseed_rewinds_the_stream() {
        let mut generator = NoiseGenerator::new(NoiseConfig {
            kind: NoiseKind::White,
            seed: 99,
            ..NoiseConfig::default()
        })
        .unwrap();

        let mut first = [0.0f32; 64];
        generator.fill_mono(&mut first);
        generator.reseed(99);

        let mut second = [0.0f32; 64];
        generator.fill_mono(&mut second);

        assert_eq!(first, second);
    }
}
