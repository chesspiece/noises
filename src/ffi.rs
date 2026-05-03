use std::{ptr, slice};

use crate::{NoiseConfig, NoiseGenerator, NoiseKind};

/// Allocates a generator and returns an opaque pointer for FFI callers.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_new(config: NoiseConfig) -> *mut NoiseGenerator {
    match NoiseGenerator::new(config) {
        Ok(generator) => Box::into_raw(Box::new(generator)),
        Err(_) => ptr::null_mut(),
    }
}

/// Frees a generator returned by [`noises_generator_new`].
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_free(generator: *mut NoiseGenerator) {
    if generator.is_null() {
        return;
    }

    // SAFETY: the pointer came from Box::into_raw in noises_generator_new and
    // is consumed at most once by this function.
    unsafe {
        drop(Box::from_raw(generator));
    }
}

/// Fills an interleaved `f32` buffer with freshly generated audio samples.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_fill_f32(
    generator: *mut NoiseGenerator,
    output: *mut f32,
    len: usize,
) -> bool {
    if generator.is_null() || output.is_null() {
        return false;
    }

    // SAFETY: pointers are validated for null above, and the caller provides
    // ownership of a writable buffer of `len` `f32` samples.
    let generator = unsafe { &mut *generator };
    // SAFETY: output is non-null and points to `len` writable `f32`s.
    let output = unsafe { slice::from_raw_parts_mut(output, len) };
    generator.fill_interleaved(output);
    true
}

/// Fills an interleaved signed PCM Q0.15 buffer with freshly generated samples.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_fill_q015(
    generator: *mut NoiseGenerator,
    output: *mut i16,
    len: usize,
) -> bool {
    if generator.is_null() || output.is_null() {
        return false;
    }

    // SAFETY: pointers are validated for null above, and the caller provides
    // ownership of a writable buffer of `len` `i16` samples.
    let generator = unsafe { &mut *generator };
    // SAFETY: output is non-null and points to `len` writable `i16`s.
    let output = unsafe { slice::from_raw_parts_mut(output, len) };
    generator.fill_interleaved_q015(output);
    true
}

/// Generates a single mono sample.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_next_mono(generator: *mut NoiseGenerator) -> f32 {
    if generator.is_null() {
        return 0.0;
    }

    // SAFETY: the pointer is non-null and treated as uniquely borrowed for the
    // duration of the call.
    let generator = unsafe { &mut *generator };
    generator.next_mono()
}

/// Generates a single mono signed PCM Q0.15 sample.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_next_mono_q015(generator: *mut NoiseGenerator) -> i16 {
    if generator.is_null() {
        return 0;
    }

    // SAFETY: the pointer is non-null and treated as uniquely borrowed for the
    // duration of the call.
    let generator = unsafe { &mut *generator };
    generator.next_mono_q015()
}

/// Switches the active noise color.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_set_kind(
    generator: *mut NoiseGenerator,
    kind: NoiseKind,
) -> bool {
    if generator.is_null() {
        return false;
    }

    // SAFETY: the pointer is non-null and uniquely borrowed for this call.
    let generator = unsafe { &mut *generator };
    generator.set_kind(kind);
    true
}

/// Updates the output amplitude.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_set_amplitude(
    generator: *mut NoiseGenerator,
    amplitude: f32,
) -> bool {
    if generator.is_null() {
        return false;
    }

    // SAFETY: the pointer is non-null and uniquely borrowed for this call.
    let generator = unsafe { &mut *generator };
    generator.set_amplitude(amplitude).is_ok()
}

/// Resets the generator to a specific seed.
#[unsafe(no_mangle)]
pub extern "C" fn noises_generator_reseed(generator: *mut NoiseGenerator, seed: u64) -> bool {
    if generator.is_null() {
        return false;
    }

    // SAFETY: the pointer is non-null and uniquely borrowed for this call.
    let generator = unsafe { &mut *generator };
    generator.reseed(seed);
    true
}
