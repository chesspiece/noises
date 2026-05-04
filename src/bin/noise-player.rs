#![cfg(feature = "cpal-demo")]

use std::{env, error::Error, io, thread, time::Duration};

use cpal::{
    FromSample, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use noises::{NoiseConfig, NoiseGenerator, NoiseKind};

const USAGE: &str = "\
Usage:
  cargo run --features cpal-demo --bin noise-player -- [options]

Options:
  --kind <white|pink|brown>   Noise color (default: pink)
  --playback <f32|q015>       Generator playback path (default: f32)
  --amplitude <0.0..1.0>      Output gain (default: 0.2)
  --sample-rate <hz>          Override the device sample rate
  --channels <n>              Override the device channel count
  --seed <u64>                RNG seed for deterministic testing
  --seconds <n>               Stop automatically after n seconds
  --device <name>             Use a specific output device by exact name
  --list-devices              Print output devices and exit
  --help                      Show this message

Notes:
  - Windows uses the host CPAL exposes, which is typically WASAPI.
  - On Linux, CPAL plays through the default audio device it sees; on PipeWire
    systems that is commonly the system sink exposed through ALSA/JACK.
";

fn main() -> Result<(), Box<dyn Error>> {
    let options = CliOptions::parse(env::args().skip(1))?;
    let host = cpal::default_host();

    if options.list_devices {
        list_devices(&host)?;
        return Ok(());
    }

    let device = match options.device_name.as_deref() {
        Some(name) => find_output_device(&host, name)?
            .ok_or_else(|| format!("output device not found: {name}"))?,
        None => host
            .default_output_device()
            .ok_or("no default output device available")?,
    };

    let device_name = device.name()?;
    let default_config = device.default_output_config()?;
    println!(
        "Sample format: {}",
        default_config.sample_format().to_string()
    );
    let sample_format = default_config.sample_format();

    let mut config: StreamConfig = default_config.into();
    if let Some(sample_rate) = options.sample_rate {
        config.sample_rate.0 = sample_rate;
    }
    if let Some(channels) = options.channels {
        config.channels = channels;
    }

    let generator = NoiseGenerator::new(NoiseConfig {
        kind: options.kind,
        sample_rate: config.sample_rate.0,
        channels: config.channels,
        amplitude: options.amplitude,
        seed: options.seed,
    })?;

    eprintln!(
        "playing {:?} noise ({}) on \"{}\" at {} Hz / {} channels",
        options.kind,
        options.playback_format.label(),
        device_name,
        config.sample_rate.0,
        config.channels
    );

    let stream = build_stream(
        &device,
        &config,
        sample_format,
        options.playback_format,
        generator,
    )?;
    stream.play()?;

    if let Some(seconds) = options.seconds {
        thread::sleep(Duration::from_secs_f32(seconds));
    } else {
        eprintln!("press Enter to stop");
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CliOptions {
    kind: NoiseKind,
    playback_format: PlaybackFormat,
    amplitude: f32,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    seed: u64,
    seconds: Option<f32>,
    device_name: Option<String>,
    list_devices: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            kind: NoiseKind::Pink,
            playback_format: PlaybackFormat::F32,
            amplitude: 0.2,
            sample_rate: None,
            channels: None,
            seed: NoiseConfig::default().seed,
            seconds: None,
            device_name: None,
            list_devices: false,
        }
    }
}

impl CliOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--kind" => {
                    let value = next_value(&mut args, "--kind")?;
                    options.kind = parse_kind(&value)?;
                }
                "--playback" => {
                    let value = next_value(&mut args, "--playback")?;
                    options.playback_format = parse_playback_format(&value)?;
                }
                "--amplitude" => {
                    let value = next_value(&mut args, "--amplitude")?;
                    options.amplitude = value.parse()?;
                }
                "--sample-rate" => {
                    let value = next_value(&mut args, "--sample-rate")?;
                    options.sample_rate = Some(value.parse()?);
                }
                "--channels" => {
                    let value = next_value(&mut args, "--channels")?;
                    options.channels = Some(value.parse()?);
                }
                "--seed" => {
                    let value = next_value(&mut args, "--seed")?;
                    options.seed = value.parse()?;
                }
                "--seconds" => {
                    let value = next_value(&mut args, "--seconds")?;
                    options.seconds = Some(value.parse()?);
                }
                "--device" => {
                    options.device_name = Some(next_value(&mut args, "--device")?);
                }
                "--list-devices" => options.list_devices = true,
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument: {arg}\n\n{USAGE}").into()),
            }
        }

        Ok(options)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackFormat {
    F32,
    Q015,
}

impl PlaybackFormat {
    const fn label(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::Q015 => "Q0.15",
        }
    }
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn parse_kind(value: &str) -> Result<NoiseKind, Box<dyn Error>> {
    match value {
        "white" => Ok(NoiseKind::White),
        "pink" => Ok(NoiseKind::Pink),
        "brown" => Ok(NoiseKind::Brown),
        _ => Err(format!("invalid noise kind: {value}").into()),
    }
}

fn parse_playback_format(value: &str) -> Result<PlaybackFormat, Box<dyn Error>> {
    match value {
        "f32" => Ok(PlaybackFormat::F32),
        "q015" | "pcm16" => Ok(PlaybackFormat::Q015),
        _ => Err(format!("invalid playback format: {value}").into()),
    }
}

fn list_devices(host: &cpal::Host) -> Result<(), Box<dyn Error>> {
    for device in host.output_devices()? {
        println!("{}", device.name()?);
    }
    Ok(())
}

fn find_output_device(
    host: &cpal::Host,
    name: &str,
) -> Result<Option<cpal::Device>, Box<dyn Error>> {
    for device in host.output_devices()? {
        if device.name()?.as_str() == name {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    playback_format: PlaybackFormat,
    generator: NoiseGenerator,
) -> Result<Stream, Box<dyn Error>> {
    let stream = match (playback_format, sample_format) {
        (PlaybackFormat::F32, SampleFormat::F32) => {
            build_stream_f32_inner::<f32>(device, config, generator)?
        }
        (PlaybackFormat::F32, SampleFormat::I16) => {
            build_stream_f32_inner::<i16>(device, config, generator)?
        }
        (PlaybackFormat::F32, SampleFormat::U16) => {
            build_stream_f32_inner::<u16>(device, config, generator)?
        }
        (PlaybackFormat::Q015, SampleFormat::F32) => {
            build_stream_q015_inner::<f32>(device, config, generator)?
        }
        (PlaybackFormat::Q015, SampleFormat::I16) => {
            build_stream_q015_inner::<i16>(device, config, generator)?
        }
        (PlaybackFormat::Q015, SampleFormat::U16) => {
            build_stream_q015_inner::<u16>(device, config, generator)?
        }
        _ => return Err(format!("unsupported sample format: {sample_format:?}").into()),
    };

    Ok(stream)
}

fn build_stream_f32_inner<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut generator: NoiseGenerator,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let err_fn = |err| eprintln!("stream error: {err}");
    let mut scratch = Vec::<f32>::new();

    device.build_output_stream(
        config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            if scratch.len() != data.len() {
                scratch.resize(data.len(), 0.0);
            }

            generator.fill_interleaved(&mut scratch);

            for (sample, value) in data.iter_mut().zip(scratch.iter().copied()) {
                *sample = T::from_sample(value);
            }
        },
        err_fn,
        None,
    )
}

fn build_stream_q015_inner<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut generator: NoiseGenerator,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<i16>,
{
    let err_fn = |err| eprintln!("stream error: {err}");
    let mut scratch = Vec::<i16>::new();

    device.build_output_stream(
        config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            if scratch.len() != data.len() {
                scratch.resize(data.len(), 0);
            }

            generator.fill_interleaved_q015(&mut scratch);

            for (sample, value) in data.iter_mut().zip(scratch.iter().copied()) {
                *sample = T::from_sample(value);
            }
        },
        err_fn,
        None,
    )
}
