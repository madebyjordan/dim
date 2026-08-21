# Dim Nightfall patches

This directory vendors Nightfall tag `0.3.12-rc4` at commit
`147ea96146b4cae6f666741020cef0622a90d46c`. Dim changes Nightfall's obsolete
`hls_ts_options` FFmpeg argument to `hls_segment_options`, matching FFmpeg 6 and newer without
pulling in Nightfall's unrelated dependency or profile changes.

Dim additionally preserves FFmpeg's original version-1 `sidx` bytes while Nightfall updates fMP4
segment sequence numbers. The `mp4` revision used by Nightfall serializes that box 14 bytes short,
which corrupts FFmpeg 8 output and prevents DASH playback from starting.

Milestone 1 adds standalone adapter guardrails. Eclipse commits Nightfall's independent lockfile,
pins the patched `mp4-rust` revision in the manifest, and validates formatting, unit tests, and
Clippy on Linux, macOS, and Windows. The real FFmpeg 9 media exercise remains ignored during fast
tests and runs only through its manual acceptance workflow. The initial strict-lint exceptions and
legacy adapter debt were intentionally left for the later runtime and dependency milestones.

Milestone 2 publishes patched init and media artifacts into immutable, generation-scoped paths.
Patchers write a sibling temporary file and make it visible only after the complete output has been
flushed, so repeated and range requests reuse identical bytes and never truncate a file being read.
Patch, copy, process-wait, successful-exit-without-output, and cancellation outcomes now reach
callers as terminal results. Child processes are consistently reaped, Windows process handles are
closed, AMF is classified as hardware acceleration, VAAPI validates concrete encode/decode
entrypoints and width/height order, and CUDA/VAAPI registration is limited to Linux. This milestone
does not replace the legacy fMP4 parser; that remains Milestone 4 work.

Milestone 3 replaces Nightfall's process-wide progress map and actor polling with retained, typed
per-session lifecycle, progress, reset, and output events. FFmpeg completion is awaited directly,
and Dim's init, segment, and subtitle waiters subscribe before checking readiness, so a change that
wins the check-to-await race is retained instead of waiting for a 100 ms retry tick. The state
manager is now a short-lived registry of independently ordered session mutexes: publication,
process cancellation/reaping, and filesystem cleanup for one session do not occupy a global actor
or block unrelated sessions. Completed stderr history is bounded, while progress and terminal
watch state are released with their session. Deterministic tests cover event ordering and delivery,
terminal wakeups, race-safe subscription, immutable publication, and cross-session isolation.

Milestone 4 replaces the production full-file fMP4 parser with a bounded-memory parse/patch/copy
engine. It validates short, extended, and final zero-sized headers with 64 GiB fragment, 64 MiB
control-box, 64 KiB brand-box, eight-level nesting, 16,384-box, 4,096-segment, and 1,000,000-sample
limits; all offset, table-width, sequence, SIDX-reference, and TFDT conversion arithmetic is
checked. Version-0/1 SIDX and TFDT fields keep their specified widths, including FFmpeg 8/9
version-1 timestamps. The complete input is validated before publication, `mdat` is copied through
a fixed 1 MiB window, and fully-synced temporary files are linked into generation-scoped paths with
atomic create-if-absent semantics. Identical retries succeed; non-identical destinations are never
replaced. Corpus tests cover malformed/truncated/overflowed headers, extended and zero sizes,
limits, idempotence, partial init signaling, legacy-compatible public fixtures, and large sparse
payloads. `PATCH_BENCHMARK.md` records reproducible before/after wall-time and peak-memory evidence.

Milestone 5 replaces raw `Option<Vec<String>>` profile output with typed, validated FFmpeg command
and representation contracts. One shared fMP4/HLS mux builder now owns init/segment/playlist paths,
discontinuity flags, muxer format, segment cadence, progress output, and temporary-file publication
flags for audio and all software/hardware video profiles. Integer microsecond/nanosecond contracts
reject invalid codecs, paths, dimensions, bitrates, sample rates, NaN/infinite/negative timing,
timeline/cadence mismatch, non-frame-aligned CFR segments, seek/window overflow, and publication
generation overflow before spawn. Hardware commands declare and validate their ordered software
fallback.

FFmpeg stderr is drained concurrently into a fixed 64 KiB tail ring instead of an unbounded file;
completed history retains 64 capped entries. Publication reset keeps four immutable generations
with constant-work pruning and fails safely if an expired generation cannot be removed. The unused
Nightfall ffprobe parser, process-liveness probe, ignored profile-init argument, actor dependencies,
legacy derive crates, and obsolete platform crates are removed. Nightfall now uses Rust 2021,
`thiserror` 2, UUID 1, `std::sync::OnceLock`, `libc` Unix signalling, and `windows-sys` process and
atomic-replacement APIs. VAAPI remains a Linux-only optional dependency; CUDA registration remains
Linux-only; macOS and Windows portable feature sets do not resolve unavailable VAAPI libraries.
