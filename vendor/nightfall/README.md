<h1>Nightfall</h1>

Nightfall is a library used internally by [Dim](https://github.com/vgarleanu/dim) to allow on-demand transcoding and streaming of various video files.

# Features
1. Transmuxing/Transcoding to DASH/fmp4.
2. Throttling of ffmpeg.
3. Subtitle streaming

# Dependencies
`ffmpeg` and `ffprobe` >= 9.0

# Validation

Nightfall is excluded from Eclipse's Cargo workspace, so validate it through its standalone
manifest and committed lockfile:

```sh
pnpm nightfall:fmt
pnpm nightfall:test
pnpm nightfall:clippy
```

The portable test and Clippy commands cover the common adapter plus CUDA/SSA argument generation.
CI additionally exercises VAAPI on Linux, the native process-state implementation on macOS, and
AMF/process handling on Windows. When `Cargo.toml` changes, refresh `vendor/nightfall/Cargo.lock`
deliberately and include both files in the same change.

The ignored real-media test is intentionally separate from fast validation. Run the
`Nightfall FFmpeg 9 acceptance` workflow manually, or provision `utils/ffmpeg` and
`utils/ffprobe` with `scripts/install-ffmpeg9-linux.sh` before invoking the exact ignored test
documented in that workflow.
