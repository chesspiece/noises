# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust crate for continuous white, pink, and brown noise generation. Keep the DSP core backend-agnostic so it can be embedded into WinUI, Slint, terminal apps, and future web builds.

- `src/lib.rs`: public crate entry point and top-level docs
- `src/noise.rs`: generator state, noise algorithms, and unit tests
- `src/ffi.rs`: C ABI for interop with C# or other native hosts
- `src/bin/noise-player.rs`: optional `cpal`-based playback binary for local testing
- `target/`: build artifacts, ignored by Git

## Build, Test, and Development Commands
Use Cargo for all routine work.

- `cargo fmt`: format the crate
- `cargo test`: run library unit tests and doctests
- `cargo build --release`: build the library as `rlib` and `cdylib`
- `cargo build --features cpal-demo --bin noise-player`: build the terminal player
- `cargo run --features cpal-demo --bin noise-player -- --kind pink`: play pink noise through the default output device

## Coding Style & Naming Conventions
Follow `rustfmt` defaults: 4-space indentation, trailing commas where useful, and idiomatic Rust naming. Use `snake_case` for files, modules, and functions; `CamelCase` for types; `SCREAMING_SNAKE_CASE` for constants. Keep public APIs documented and small. In audio paths, avoid unnecessary allocations and keep callback work predictable.

## Testing Guidelines
Prefer deterministic tests by setting explicit seeds. Add unit tests near the implementation in `src/noise.rs` unless a larger integration test justifies `tests/`. Verify that generators stay finite, bounded, reproducible, and continuous. Any change to playback or interop should include either automated coverage or a short manual verification note.

## Architecture Notes
Do not add looping audio assets for core playback. Noise must be generated continuously at runtime. Keep the library independent from GUI frameworks; adapters for Slint, WinUI, or WASM should sit above the shared generator API.

## Commit & Pull Request Guidelines
This repository does not have commit history yet, so use Conventional Commit style from now on, for example `feat: add wasm wrapper` or `fix: clamp brown noise output`. PRs should include a concise summary, test commands run, and any platform notes for Windows or Linux audio behavior.

## Security & Configuration Tips
Do not commit secrets, local audio-device settings, or generated binaries. Keep machine-specific configuration out of source control.
