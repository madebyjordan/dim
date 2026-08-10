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
yarn build
yarn dev
```

Before submitting a change, run:

```sh
cargo fmt --all --check
cargo test --workspace --tests --locked
cd ui && corepack yarn prettier --check src && corepack yarn eslint --ext .js,.jsx,.ts,.tsx src
```

## Contributors
Valerian G. (Lead Developer and maintainer) \
[Valerian G.](https://github.com/vgarleanu)
[eraychumal](https://github.com/eraychumak)
[mental32](https://github.com/mental32)
[IGI-111](https://github.com/igi-111)
