## Contributing
Contributions are absolutely, positively welcome and encouraged! Contributions
come in many forms. You could:

  1. Submit a feature request or bug report as an [issue].
  2. Ask for improved documentation as an [issue].
  3. Contribute code via [pull requests].

[issue]: https://github.com/Dusk-Labs/dim/issues
[pull_requests]: https://github.com/Dusk-Labs/dim/pulls

All pull requests are code reviewed and tested by the CI. Note that unless you
explicitly state otherwise, any contribution intentionally submitted for
inclusion in dim by you shall be licensed under the AGPLv3 License 
without any additional terms or conditions.

### Local development

Follow the source prerequisites in [README.md](README.md), then bootstrap a development build with:

```sh
pnpm build
pnpm dev
```

The debug `pnpm dev` command starts both Rust and the SvelteKit/Vite development server. Open
[http://localhost:5173](http://localhost:5173) for frontend work; Vite hot-updates Svelte components
and CSS, and proxies backend, playback, image, and WebSocket traffic to Rust on port 8000. It does
not regenerate `eclipse/build`. Release bundles are embedded at Rust compile time, so use
`pnpm build --release` before `pnpm dev --release`.

Before submitting a change, run:

```sh
pnpm test
cargo fmt --all --check
corepack pnpm --dir eclipse exec prettier --check src
corepack pnpm --dir eclipse check
```

The commands after `pnpm test` format and type-check the SvelteKit frontend. Normal branch CI
additionally runs the two legacy scanner tests excluded from the deterministic root test command
and release gate.

## Contributors
Valerian G. (Lead Developer and maintainer) \
[Valerian G.](https://github.com/vgarleanu)
[eraychumal](https://github.com/eraychumak)
[mental32](https://github.com/mental32)
[IGI-111](https://github.com/igi-111)
