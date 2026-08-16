<h1 align="center">Dim</h1>

![Dashboard](docs/design/dashboard.jpg)
[![Discord](https://img.shields.io/discord/834495310332035123)](https://discord.gg/gBPyQ7NVah)

## 2026

Dim is a project with a lot of potential, I said this back in 2021-2022 when I was broughr onto the project as the sole designer at the time and now I am taking the opportunity to continue work on Dim, focusing on improving its foundation and overall user experience, making it more broadly accessible to a wider audience.

Dim is a self-hosted media manager. With minimal setup, Dim will organize and beautify your media collections for playback on localhost or a trusted home network.

## Running from source

Dim's supported source-development path is macOS or Linux. The repository pins Rust in
`rust-toolchain.toml`, Node.js in `.nvmrc`, pnpm in the root `package.json`, and Yarn 4 in
`ui/package.json` for UI-internal tooling.

### Prerequisites

- Git
- Rust 1.93.1, installed automatically through [rustup](https://rustup.rs/)
- Node.js 24.19.0 LTS, with Corepack available
- pnpm 11.9.0, activated from the root `package.json` with Corepack
- FFmpeg and FFprobe 6.0 or newer
- SQLite development tools
- A C/C++ build toolchain and `pkg-config`
- Linux only: OpenSSL development headers
- Linux VAAPI builds only: `libva-dev`, `libva-drm2`, and `libva2`

On macOS with Homebrew, the native dependencies can be installed with:

```sh
brew install ffmpeg sqlite pkg-config
```

On Debian or Ubuntu:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev sqlite3 ffmpeg
```

Enable the repository-pinned pnpm command once after installing Node.js:

```sh
corepack enable pnpm
```

The bootstrap script invokes the UI's pinned Yarn 4 version through Corepack directly, so Yarn
does not need to be installed globally and its global shim does not need to be enabled.

### Build and run

From a fresh clone:

```sh
git clone https://github.com/Dusk-Labs/dim.git
cd dim
pnpm build
pnpm dev
```

Open [http://localhost:8000](http://localhost:8000) after Dim starts. To build and run an optimized binary, pass `--release` to both commands:

```sh
pnpm build --release
pnpm dev --release
```

The bootstrap script installs the locked frontend dependencies, builds the embedded web UI, links the system FFmpeg tools under `utils/`, and builds the Rust workspace. It never overwrites existing FFmpeg binaries in `utils/`.

### Test

From the repository root, run the release-command, frontend, and reliable locked Rust workspace
test suites with:

```sh
pnpm test
```

Use `pnpm test:release` when working only on the automated release command. The complete release
gate, including formatting, contract, type, lint, test, and optimized-build validation, is:

```sh
pnpm release:validate
```

Dim creates local state relative to its runtime directory. `scripts/run.sh` uses the repository root for development builds and `target/release/` for release builds, matching the layout of packaged binaries:

| Path                 | Purpose                                               |
| -------------------- | ----------------------------------------------------- |
| `config/config.toml` | Host configuration and generated local session secret |
| `config/dim.db`      | SQLite database                                       |
| `metadata/`          | Downloaded artwork and metadata                       |
| `streaming_cache/`   | Temporary playback and transcoding data               |

Existing configuration files are preserved. If upgrading from a version that exposed `secret_key`, remove that setting once and restart Dim to generate a new secret and invalidate existing sessions.

Dim listens on `127.0.0.1` by default. Trusted LAN access is opt-in with an explicit listener, for example `dim --bind-address 0.0.0.0` or `bind_address = "0.0.0.0"` in the configuration file. The effective listener is logged at startup.

Direct internet exposure is unsupported. Dim has no built-in TLS termination; use an appropriately configured trusted reverse proxy when HTTPS access is intentional. See [the deployment and authentication boundary](docs/design/deployment-security.md) for proxy settings, session migration behavior, and security limits.

## Automated releases

`Cargo.toml`'s `workspace.package.version` is the single application-version source. Every Dim
workspace crate inherits it, and Cargo synchronizes `Cargo.lock` when the release command changes
the version.

The root release commands validate, version, commit, push, and tag the synchronized release branch.
The pushed tag automatically starts `.github/workflows/release.yml`, which builds and publishes the
Linux archive, checksum, container tags, and GitHub Release. There is no separate manual version,
tag, or GitHub Release step.

The first fork release is deliberately bootstrapped as `v0.3.0`:

```sh
pnpm release:initial -- --dry-run
pnpm release:initial
```

After `v0.3.0` exists, choose the semantic-version bump explicitly:

```sh
pnpm release:patch -- --dry-run
pnpm release:patch
pnpm release:minor -- --dry-run
pnpm release:minor
pnpm release:major -- --dry-run
pnpm release:major
```

The command must run on `master` tracking `origin/master`, with authenticated `gh` and Git access to
`origin`, Node/Corepack, pnpm, Rust, FFmpeg/FFprobe, and the native build dependencies listed above.
Pending tracked changes, non-ignored untracked files, and local commits ahead of `origin/master` are
valid release inputs. Remote-only commits or a diverged branch abort; the command never pulls,
merges, rebases, force-pushes, or overwrites remote history. Set `DIM_RELEASE_BRANCH` or
`DIM_RELEASE_REMOTE` only when the fork's intended release branch or remote is intentionally
different.

Before mutation it fetches tags, verifies remote ancestry and tag/Release availability, reports
every file that will enter the release commit, and runs the same frontend and Rust validation used
by the release workflow. A real release updates `Cargo.toml` and `Cargo.lock`, stages the current
non-ignored project changes, and creates one `chore: release vX.Y.Z` commit. It pushes `master`
without force and verifies the remote commit before creating an annotated tag on that exact commit
and pushing the tag separately. The one tag-triggered workflow builds the Linux x86_64 archive and
checksum, publishes versioned Linux x86_64 GHCR tags, and creates the GitHub Release. Dry runs
execute the complete inspection and validation path but never modify files or the index, create a
commit or tag, push, or publish anything.

The release gate excludes two legacy scanner tests that can wait indefinitely on metadata/probe
work: `test_construct_mediafile` and
`rescan_keeps_metadata_aligned_after_existing_files_are_filtered`. They remain enabled in normal
branch CI; every other locked workspace test runs before a release can be committed or tagged.

## Running from binaries

### Dependencies

- libva2
- libva-drm2
- libharfbuzz
- libfontconfig
- libfribidi
- libtheora
- libvorbis
- libvorbisenc
- libtheora0

Download `dim-vX.Y.Z-linux-x86_64.tar.gz` and its `.sha256` file from the fork's GitHub Release,
then verify and unpack it:

1. Verify with `sha256sum -c dim-vX.Y.Z-linux-x86_64.tar.gz.sha256`.
2. Unpack with `tar -xvzf dim-vX.Y.Z-linux-x86_64.tar.gz`.
3. Run `cd release && ./dim`.
4. Access the Dim web UI at `http://localhost:8000`.

## Running with docker

The following command runs dim on port 8000, storing configuration in `$HOME/.config/dim`.
You may change that path if you'd like to store configuration somewhere else.
You can mount as many directories containing media as you like by repeating the `-v HOST_PATH:CONTAINER_PATH` option.
In this example, the path `/media` on the host is made available at the same path inside the Docker container.
This name "media" is arbitrary and you can choose whatever you like.

```
docker run -d -p 127.0.0.1:8000:8000/tcp -e DIM_BIND_ADDRESS=0.0.0.0 -v $HOME/.config/dim:/opt/dim/config -v /media:/media:ro ghcr.io/dusk-labs/dim:dev
```

The multi-architecture image resides at `ghcr.io/dusk-labs/dim:master`.

To use hardware acceleration, mount the relevant device:

```
docker run -d -p 127.0.0.1:8000:8000/tcp -e DIM_BIND_ADDRESS=0.0.0.0 -v $HOME/.config/dim:/opt/dim/config -v /media:/media:ro --device=/dev/dri/renderD128 ghcr.io/dusk-labs/dim:dev
```

Refer to [docker-compose-template.yml](docker-compose-template.yml) to run Dim using Docker Compose.

## License

Dim is licensed under the AGPLv3 license (see [LICENSE.md](LICENSE.md) or https://opensource.org/licenses/AGPL-3.0)

## Screenshots

![Login_Page](docs/design/login_page.png)
![Add_Library Modal](docs/design/add_library.png)
![Media_Page](docs/design/media_page.jpg)
