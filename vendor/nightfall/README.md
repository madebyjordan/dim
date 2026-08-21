<h1>Nightfall</h1>

Nightfall is a library used internally by [Dim](https://github.com/vgarleanu/dim) to allow on-demand transcoding and streaming of various video files.

# Features
1. Transmuxing/transcoding to fragmented-MP4 HLS.
2. Throttling of ffmpeg.
3. Subtitle streaming

# Dependencies
Nightfall executes `ffmpeg` >= 9.0. Eclipse owns media probing separately and validates its
`ffprobe` binary at application startup.

# Validation

Nightfall is excluded from Eclipse's Cargo workspace, so validate it through its standalone
manifest and committed lockfile:

```sh
pnpm nightfall:fmt
pnpm nightfall:test
pnpm nightfall:clippy
```

The production fMP4 patcher uses bounded metadata plans and a 1 MiB streaming copy window instead
of materializing `mdat`. See [`PATCH_BENCHMARK.md`](PATCH_BENCHMARK.md) for the reproducible legacy
comparison, wall-time and peak-RSS evidence, and the small-fragment tradeoff.

Built-in profiles produce a typed `FfmpegCommand` and `Representation` before process spawn. The
contract validates codec/container pairing, generated output paths, dimensions, integer
microsecond/nanosecond timing, CFR frame cadence, seek/window arithmetic, and ordered
hardware-to-software fallback. FFmpeg stderr is drained incrementally into a 64 KiB tail ring;
terminal history retains at most 64 diagnostics and sessions retain four immutable publication
generations.

The portable test and Clippy commands cover the common adapter plus platform-appropriate CUDA/SSA
argument generation. CI additionally exercises VAAPI and CUDA on Linux, the native process-state
implementation with hardware profiles gated off on macOS, and AMF/process handling on Windows.
When `Cargo.toml` changes, refresh `vendor/nightfall/Cargo.lock` deliberately and include both files
in the same change.

Session consumers should call `StateManager::subscribe` before their first readiness request and
await `SessionSubscription::changed` only after `ChunkNotDone`. Each subscription retains the
latest typed lifecycle, progress, reset, or output event, preventing the check-to-await race without
fixed-interval polling. Operations remain ordered within a session but run independently across
sessions.

The ignored real-media test is intentionally separate from fast validation. Run the
`Nightfall FFmpeg 9 acceptance` workflow manually, or provision `utils/ffmpeg` and
`utils/ffprobe` with `scripts/install-ffmpeg9-linux.sh` before invoking the exact ignored test
documented in that workflow.
