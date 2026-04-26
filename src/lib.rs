#![forbid(unsafe_op_in_unsafe_fn)]
#![doc = r#"
Continuous runtime noise generators for white, pink, and brown noise.

The core library is audio-backend agnostic: it only produces interleaved `f32`
samples, so the same generator can feed a `cpal` output stream, a Slint app,
WinUI via FFI/PInvoke, or a browser-side WASM wrapper. The stream is generated
sample-by-sample at runtime and never loops pre-rendered audio.

```rust
use noises::{NoiseConfig, NoiseGenerator, NoiseKind};

let mut generator = NoiseGenerator::new(NoiseConfig::new(NoiseKind::Pink)).unwrap();
let mut stereo_buffer = [0.0f32; 512];
generator.fill_interleaved(&mut stereo_buffer);
```
"#]

mod ffi;
mod noise;

pub use noise::{NoiseConfig, NoiseError, NoiseGenerator, NoiseKind};
