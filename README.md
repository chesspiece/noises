# noises

`noises` was made with the help of openai codex.

It was created for two reasons:

1. to test vibecoding as a way to build something real and useful
2. to have good continuous noise for drowning out distractions while working

The project is a Rust library for generating continuous white, pink, and brown noise at runtime.

## Current features

- White, pink, and brown noise generation
- Continuous runtime generation
- Rust library outputting `f32` audio samples or signed PCM Q0.15 `i16` samples
- C-compatible FFI layer for native app integration
- Optional `cpal` playback binary for local testing on desktop systems

## Quick start

Build and test the library:

```bash
cargo test
```

Run the terminal player:

```bash
cargo run --features cpal-demo --bin noise-player -- --kind pink
```

Use the signed PCM Q0.15 generator path in the terminal player:

```bash
cargo run --features cpal-demo --bin noise-player -- --kind pink --playback q015
```

Try other noise colors:

```bash
cargo run --features cpal-demo --bin noise-player -- --kind white
cargo run --features cpal-demo --bin noise-player -- --kind brown
```

List audio output devices:

```bash
cargo run --features cpal-demo --bin noise-player -- --list-devices
```

## Project layout

- `src/noise.rs`: core DSP and generator state
- `src/ffi.rs`: C ABI for interop
- `src/lib.rs`: public crate entry point
- `src/bin/noise-player.rs`: optional local playback demo
