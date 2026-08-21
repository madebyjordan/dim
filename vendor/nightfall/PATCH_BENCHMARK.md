# Nightfall fMP4 patch benchmark

Milestone 4 replaces the production full-file parser with a bounded streaming patcher. The
`patch_benchmark` example generates deterministic, valid minimal fMP4 media or init-with-embedded-
media fragments without allocating the payload. Its `legacy-media` and `legacy-init` modes retain
the pre-Milestone-4 algorithm for reproducible comparison only; production entrypoints use the new
engine.

## Reproduce

Build the optimized harness with the same portable feature set used by Nightfall CI:

```sh
cargo build --release --manifest-path vendor/nightfall/Cargo.toml --locked \
  --example patch_benchmark --no-default-features --features cuda,ssa_transmux
```

On macOS, capture internal patch time and process peak RSS with:

```sh
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark legacy-media 262144 25
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark media        262144 25
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark legacy-init 262144 25
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark init        262144 25

/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark legacy-media 134217728 3
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark media        134217728 3
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark legacy-init  134217728 3
/usr/bin/time -l vendor/nightfall/target/release/examples/patch_benchmark init         134217728 3
```

Use `/usr/bin/time -v` instead of `-l` on Linux. The harness reports `mean_patch_ms`; external time
also includes deterministic input creation and cleanup. Run on a quiet local filesystem and keep
the payload sizes and iteration counts unchanged when comparing results.

## Milestone 4 evidence

Measured on 2026-08-21 at Eclipse commit `f7ff12cafdda5b728b7355e2310c1a5e38d25bd0`
plus the saved Milestone 2/3 worktree, using Rust 1.93.1 release builds, macOS 26.6 (Darwin 25.6.0),
an Apple M4 Pro with 24 GiB RAM, and the system temporary APFS volume. Peak RSS is the whole
benchmark process, including its 1 MiB fixed copy window.

| Fragment | Payload | Legacy mean | Streaming mean | Time change | Legacy peak RSS | Streaming peak RSS | RSS change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| media | 256 KiB | 6.241 ms | 6.457 ms | +3.5% | 2,441,216 B | 3,325,952 B | +36.2% |
| embedded init/media | 256 KiB | 11.772 ms | 12.193 ms | +3.6% | 2,490,368 B | 3,342,336 B | +34.2% |
| media | 128 MiB | 34.741 ms | 26.454 ms | -23.9% | 136,331,264 B | 3,178,496 B | -97.7% |
| embedded init/media | 128 MiB | 41.409 ms | 31.935 ms | -22.9% | 136,396,800 B | 3,162,112 B | -97.7% |

The large-payload RSS result is effectively flat instead of scaling with `mdat`: increasing the
payload from 256 KiB to 128 MiB changed streaming peak RSS by less than 0.2 MiB. The small-fragment
RSS and latency regressions are the fixed cost of the 1 MiB copy window, strict pre-publication
validation, and atomic no-overwrite linking. Real HLS fragments are commonly much larger than the
small synthetic case; the large case demonstrates the targeted wall-time and memory improvement.

Raw local evidence is written by the managed validation commands to
`logs/codex-validation/nightfall-m4-baseline2-*.log` and
`logs/codex-validation/nightfall-m4-final-*.log`; `logs/` remains ignored.
