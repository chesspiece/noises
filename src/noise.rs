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
    pink_q015: PinkStateQ015,
    brown: BrownState,
    brown_q015: BrownStateQ015,
}

impl NoiseGenerator {
    /// Creates a new generator from a validated configuration.
    pub fn new(config: NoiseConfig) -> Result<Self, NoiseError> {
        let config = config.validate()?;
        Ok(Self {
            rng: SplitMix64::new(config.seed),
            pink: PinkState::default(),
            pink_q015: PinkStateQ015::default(),
            brown: BrownState::new(config.sample_rate),
            brown_q015: BrownStateQ015::new(config.sample_rate),
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
        self.brown_q015.set_sample_rate(sample_rate);
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
        self.pink_q015.reset();
        self.brown.reset();
        self.brown_q015.reset();
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

    /// Generates one mono signed PCM Q0.15 sample.
    ///
    /// The output range is `[-32768, 32767]`, where `-32768` represents `-1.0`
    /// and `32767` is the largest positive Q0.15 value.
    pub fn next_mono_q015(&mut self) -> i16 {
        let white = self.rng.next_signed_q015();
        let colored = match self.config.kind {
            NoiseKind::White => white,
            NoiseKind::Pink => self.pink_q015.next(white),
            NoiseKind::Brown => self.brown_q015.next(white),
        };
        scale_q015_by_amplitude(colored, self.config.amplitude)
    }

    /// Fills a mono buffer with newly generated samples.
    pub fn fill_mono(&mut self, output: &mut [f32]) {
        for sample in output {
            *sample = self.next_mono();
        }
    }

    /// Fills a mono buffer with signed PCM Q0.15 samples.
    pub fn fill_mono_q015(&mut self, output: &mut [i16]) {
        for sample in output {
            *sample = self.next_mono_q015();
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

    /// Fills an interleaved buffer with signed PCM Q0.15 samples.
    ///
    /// Each generated mono frame is duplicated into every configured channel.
    pub fn fill_interleaved_q015(&mut self, output: &mut [i16]) {
        let channels = usize::from(self.config.channels);
        for frame in output.chunks_mut(channels) {
            let sample = self.next_mono_q015();
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

    fn next_signed_q015(&mut self) -> i16 {
        (self.next_u64() >> 48) as u16 as i16
    }
}

const Q015_SCALE: i32 = 1 << 15;
const Q015_MAX: i32 = i16::MAX as i32;
const Q015_MIN: i32 = i16::MIN as i32;

const fn q015_coefficient(value: f32) -> i32 {
    let scaled = value * Q015_SCALE as f32;
    if scaled >= 0.0 {
        (scaled + 0.5) as i32
    } else {
        (scaled - 0.5) as i32
    }
}

fn mul_q015(sample: i32, coefficient: i32) -> i32 {
    ((sample as i64 * coefficient as i64) / Q015_SCALE as i64) as i32
}

fn clamp_q015(sample: i32) -> i16 {
    sample.clamp(Q015_MIN, Q015_MAX) as i16
}

fn scale_q015_by_amplitude(sample: i16, amplitude: f32) -> i16 {
    if amplitude >= 1.0 {
        sample
    } else {
        let gain = (amplitude * Q015_SCALE as f32).round() as i32;
        clamp_q015(mul_q015(i32::from(sample), gain))
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
        self.b2 = 0.969_00 * self.b2 + white * 0.153_852;
        self.b3 = 0.866_50 * self.b3 + white * 0.310_485_6;
        self.b4 = 0.550_00 * self.b4 + white * 0.532_952_2;
        self.b5 = -0.761_6 * self.b5 - white * 0.016_898_0;

        let pink =
            self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.5362;
        self.b6 = white * 0.115_926;

        (pink * 0.11).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PinkStateQ015 {
    b0: i32,
    b1: i32,
    b2: i32,
    b3: i32,
    b4: i32,
    b5: i32,
    b6: i32,
}

impl PinkStateQ015 {
    const B0_FEEDBACK: i32 = q015_coefficient(0.998_86);
    const B0_INPUT: i32 = q015_coefficient(0.055_517_9);
    const B1_FEEDBACK: i32 = q015_coefficient(0.993_32);
    const B1_INPUT: i32 = q015_coefficient(0.075_075_9);
    const B2_FEEDBACK: i32 = q015_coefficient(0.969_00);
    const B2_INPUT: i32 = q015_coefficient(0.153_852);
    const B3_FEEDBACK: i32 = q015_coefficient(0.866_50);
    const B3_INPUT: i32 = q015_coefficient(0.310_485_6);
    const B4_FEEDBACK: i32 = q015_coefficient(0.550_00);
    const B4_INPUT: i32 = q015_coefficient(0.532_952_2);
    const B5_FEEDBACK: i32 = q015_coefficient(-0.761_6);
    const B5_INPUT: i32 = q015_coefficient(-0.016_898_0);
    const DIRECT_INPUT: i32 = q015_coefficient(0.5362);
    const B6_INPUT: i32 = q015_coefficient(0.115_926);
    const OUTPUT_GAIN: i32 = q015_coefficient(0.11);

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn next(&mut self, white: i16) -> i16 {
        let white = i32::from(white);

        self.b0 = mul_q015(self.b0, Self::B0_FEEDBACK) + mul_q015(white, Self::B0_INPUT);
        self.b1 = mul_q015(self.b1, Self::B1_FEEDBACK) + mul_q015(white, Self::B1_INPUT);
        self.b2 = mul_q015(self.b2, Self::B2_FEEDBACK) + mul_q015(white, Self::B2_INPUT);
        self.b3 = mul_q015(self.b3, Self::B3_FEEDBACK) + mul_q015(white, Self::B3_INPUT);
        self.b4 = mul_q015(self.b4, Self::B4_FEEDBACK) + mul_q015(white, Self::B4_INPUT);
        self.b5 = mul_q015(self.b5, Self::B5_FEEDBACK) + mul_q015(white, Self::B5_INPUT);

        let pink = self.b0
            + self.b1
            + self.b2
            + self.b3
            + self.b4
            + self.b5
            + self.b6
            + mul_q015(white, Self::DIRECT_INPUT);
        self.b6 = mul_q015(white, Self::B6_INPUT);

        clamp_q015(mul_q015(pink, Self::OUTPUT_GAIN))
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

#[derive(Debug, Clone, Copy)]
struct BrownStateQ015 {
    value: i32,
    step: i32,
}

impl BrownStateQ015 {
    const LEAK: i32 = q015_coefficient(1.0 / 1.02);
    const OUTPUT_GAIN: i32 = q015_coefficient(3.5);

    fn new(sample_rate: u32) -> Self {
        let mut state = Self { value: 0, step: 0 };
        state.set_sample_rate(sample_rate);
        state
    }

    fn reset(&mut self) {
        self.value = 0;
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        let sample_rate = sample_rate.max(1) as f32;
        self.step = (0.02 * (48_000.0 / sample_rate).sqrt() * Q015_SCALE as f32).round() as i32;
    }

    fn next(&mut self, white: i16) -> i16 {
        let integrated = self.value + mul_q015(i32::from(white), self.step);
        self.value = mul_q015(integrated, Self::LEAK);
        clamp_q015(mul_q015(self.value, Self::OUTPUT_GAIN))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BrownStateQ015, NoiseConfig, NoiseError, NoiseGenerator, NoiseKind, PinkStateQ015,
        SplitMix64,
    };

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
    fn pink_q015_coefficients_match_float_filter_coefficients() {
        assert_eq!(PinkStateQ015::B0_FEEDBACK, 32_731);
        assert_eq!(PinkStateQ015::B0_INPUT, 1_819);
        assert_eq!(PinkStateQ015::B1_FEEDBACK, 32_549);
        assert_eq!(PinkStateQ015::B1_INPUT, 2_460);
        assert_eq!(PinkStateQ015::B2_FEEDBACK, 31_752);
        assert_eq!(PinkStateQ015::B2_INPUT, 5_041);
        assert_eq!(PinkStateQ015::B3_FEEDBACK, 28_393);
        assert_eq!(PinkStateQ015::B3_INPUT, 10_174);
        assert_eq!(PinkStateQ015::B4_FEEDBACK, 18_022);
        assert_eq!(PinkStateQ015::B4_INPUT, 17_464);
        assert_eq!(PinkStateQ015::B5_FEEDBACK, -24_956);
        assert_eq!(PinkStateQ015::B5_INPUT, -554);
        assert_eq!(PinkStateQ015::DIRECT_INPUT, 17_570);
        assert_eq!(PinkStateQ015::B6_INPUT, 3_799);
        assert_eq!(PinkStateQ015::OUTPUT_GAIN, 3_604);
    }

    #[test]
    fn brown_q015_coefficients_match_float_filter_coefficients() {
        assert_eq!(BrownStateQ015::LEAK, 32_125);
        assert_eq!(BrownStateQ015::OUTPUT_GAIN, 114_688);
    }

    #[test]
    fn white_q015_stream_uses_direct_rng_samples() {
        let seed = 55;
        let mut generator = NoiseGenerator::new(NoiseConfig {
            kind: NoiseKind::White,
            amplitude: 1.0,
            seed,
            ..NoiseConfig::default()
        })
        .unwrap();
        let mut rng = SplitMix64::new(seed);
        let mut buffer = [0i16; 256];

        generator.fill_mono_q015(&mut buffer);

        let expected = core::array::from_fn(|_| rng.next_signed_q015());
        assert_eq!(buffer, expected);
    }

    #[test]
    fn q015_streams_are_reproducible_for_all_kinds() {
        for kind in [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown] {
            let config = NoiseConfig {
                kind,
                seed: 55,
                ..NoiseConfig::default()
            };

            let mut first_generator = NoiseGenerator::new(config).unwrap();
            let mut second_generator = NoiseGenerator::new(config).unwrap();
            let mut first = [0i16; 256];
            let mut second = [0i16; 256];

            first_generator.fill_mono_q015(&mut first);
            second_generator.fill_mono_q015(&mut second);

            assert_eq!(first, second);
        }
    }

    #[test]
    fn colored_q015_streams_are_non_silent() {
        let config = NoiseConfig {
            kind: NoiseKind::Pink,
            seed: 55,
            ..NoiseConfig::default()
        };

        let mut pink = NoiseGenerator::new(config).unwrap();
        let mut brown = NoiseGenerator::new(NoiseConfig {
            kind: NoiseKind::Brown,
            ..config
        })
        .unwrap();
        let mut pink_buffer = [0i16; 8192];
        let mut brown_buffer = [0i16; 8192];

        pink.fill_mono_q015(&mut pink_buffer);
        brown.fill_mono_q015(&mut brown_buffer);

        assert!(pink_buffer.iter().any(|sample| *sample != 0));
        assert!(brown_buffer.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn q015_interleaved_fill_duplicates_each_generated_frame() {
        let mut generator = NoiseGenerator::new(NoiseConfig {
            kind: NoiseKind::White,
            channels: 3,
            seed: 8,
            ..NoiseConfig::default()
        })
        .unwrap();

        let mut buffer = [0i16; 18];
        generator.fill_interleaved_q015(&mut buffer);

        for frame in buffer.chunks_exact(3) {
            assert_eq!(frame[0], frame[1]);
            assert_eq!(frame[1], frame[2]);
        }
    }

    #[test]
    fn direct_q015_rng_is_reproducible() {
        let mut first = SplitMix64::new(3);
        let mut second = SplitMix64::new(3);

        for _ in 0..8192 {
            assert_eq!(first.next_signed_q015(), second.next_signed_q015());
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
