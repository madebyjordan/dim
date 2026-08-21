# Dim Nightfall patches

This directory vendors Nightfall tag `0.3.12-rc4` at commit
`147ea96146b4cae6f666741020cef0622a90d46c`. Dim changes Nightfall's obsolete
`hls_ts_options` FFmpeg argument to `hls_segment_options`, matching FFmpeg 6 and newer without
pulling in Nightfall's unrelated dependency or profile changes.

Dim additionally preserves FFmpeg's original version-1 `sidx` bytes while Nightfall updates fMP4
segment sequence numbers. The `mp4` revision used by Nightfall serializes that box 14 bytes short,
which corrupts FFmpeg 8 output and prevents DASH playback from starting.

Nightfall's process-state helper calls a `psutil` status API that is not implemented on macOS. Dim
uses the native Unix signal existence check on macOS so failed and completed FFmpeg sessions can be
observed and cleaned up without panicking.

Milestone 1 adds standalone adapter guardrails. Eclipse commits Nightfall's independent lockfile,
pins the patched `mp4-rust` revision in the manifest, and validates formatting, unit tests, and
Clippy on Linux, macOS, and Windows. The real FFmpeg 9 media exercise remains ignored during fast
tests and runs only through its manual acceptance workflow. Strict linting explicitly defers the
legacy `err-derive` generated-impl warning, the public error-size API debt, and VAAPI's existing
single-pass profile-probing control flow to a runtime milestone.

Milestone 3 treats this vendor directory as a compatibility adapter. Dim owns playback planning,
admission, ownership, and lifecycle outside Nightfall. The adapter propagates profile-build/spawn
failures, observes nonzero exits, ignores malformed progress lines, and removes process statistics
and output files during cancellation and shutdown. Remaining replacement work is to move fMP4
patching and process signalling behind maintained platform-neutral components.
