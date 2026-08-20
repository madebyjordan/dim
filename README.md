<h1 align="center">Eclipse</h1>

![Dashboard](docs/design/eclipse-design-home.png)
[![Discord](https://img.shields.io/discord/834495310332035123)](https://discord.gg/gBPyQ7NVah)

Eclipse is a personal media manager derived from the upstream Dim project. It retains Dim's internal Rust crate structure and the Nightfall media engine while presenting Eclipse as the product and runtime.

## Running from source

Eclipse's supported source-development path is macOS, Linux, or native Windows through Git Bash. The repository pins Rust in
`rust-toolchain.toml`, Node.js in `.nvmrc`, and pnpm in the root and Eclipse package manifests.

### Guided setup

Launch the interactive installer from the repository root. On macOS or Linux:

```sh
./install.sh
```

On Windows, use the native launcher from CMD or PowerShell:

```bat
install.cmd
```

The launcher finds Git Bash and hands off to the same shared installer. If Git for Windows is not
installed, it asks before installing `Git.Git` with WinGet. WSL is not required.

The first prompt asks you to choose macOS, Linux, or Windows. Each platform path checks the
required platform, Node, Rust, media, and native build tooling; offers focused recovery for anything
missing; runs the existing locked release bootstrap; and can start Eclipse and open
[http://localhost:8000](http://localhost:8000). Existing installations are detected before the
build starts. Reinstall/update preserves all state; Reset removes host settings, cache, and logs
while preserving accounts, libraries, progress, and metadata (and signs out existing browser
sessions when it generates a new host secret); Clean install removes managed state after explicit
confirmation. Running Eclipse processes are stopped before executable replacement.
Existing FFmpeg/FFprobe entries are never replaced. Linux can automatically install documented native requirements on Debian and
Ubuntu through `apt-get`; other distributions receive exact missing-package guidance. On Windows,
the native launcher uses Git Bash internally rather than WSL. The installer uses WinGet for
supported dependency recovery and may prompt for administrator approval when Visual Studio Build
Tools are installed.

For automation, the same entrypoint accepts `--platform macos`, `--platform linux`, `--platform
windows`, `--yes`, and `--no-start`. These options do not change the normal interactive flow.

To preview and test the complete interactive experience without inspecting the system, installing
dependencies, building Eclipse, starting processes, writing files, or opening a browser, use:

```sh
./install.sh --demo
```

From CMD or PowerShell, the equivalent is:

```bat
install.cmd --demo
```

Demo mode uses deterministic simulated results and includes the missing-requirement recovery path.
Use `--demo-scenario fresh|reinstall|reset|clean|exit` to preview every lifecycle branch. For
automation, `--existing-action reinstall|reset|clean|exit` can be combined with `--yes`.

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

On Windows, `install.cmd` can acquire Git for Windows before opening the wizard. The wizard can use
WinGet (provided by Microsoft App Installer) to recover supported packages with these identifiers:

```text
Git.Git
OpenJS.NodeJS.LTS (24.19.0)
Rustlang.Rustup
Gyan.FFmpeg
SQLite.SQLite
Microsoft.VisualStudio.2022.BuildTools
```

The Visual Studio package is installed with the `Microsoft.VisualStudio.Workload.VCTools`
workload and recommended components. Detection accepts a usable MSVC x64 compiler and Windows SDK
regardless of the Visual Studio edition or installation directory. Incomplete or inconclusive
existing installations receive Visual Studio Installer guidance rather than being modified
automatically. A newly installed package may require a new terminal before its updated `PATH` is
visible. WSL is a separate Linux environment and is not used by the native Windows installer.

The Windows CLI installer enables a user-level Corepack shim automatically, so `pnpm` is available
from newly opened CMD, PowerShell, and Git Bash sessions without elevation. For a manual source setup
that does not use the installer, enable the repository-pinned pnpm command after installing Node.js:

```sh
corepack enable pnpm
```

### Build and run

From a fresh clone:

```sh
git clone https://github.com/madebyjordan/eclipse.git
cd eclipse
pnpm build
pnpm dev
```

`pnpm build` bootstraps the Rust binary and production Eclipse bundle. `pnpm dev` then starts the
Rust backend on port 8000 and the SvelteKit/Vite development frontend at
[http://localhost:5173](http://localhost:5173). Use the Vite URL for active development: Svelte and
CSS edits update through HMR, while API, artwork, streaming, and WebSocket requests are proxied to
Rust. To build and run an optimized single-server binary, pass `--release` to both commands:

```sh
pnpm build --release
pnpm dev --release
```

The bootstrap script installs the locked frontend dependencies, builds the embedded web UI, places
the system FFmpeg tools under `utils/`, and builds the Rust workspace. Unix systems use links;
Windows uses `.exe` copies because creating symbolic links can require elevated privileges or
Developer Mode. It never overwrites existing FFmpeg binaries in `utils/`.

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

Eclipse creates local state relative to its runtime directory. `scripts/run.sh` uses the repository root for development builds and `target/release/` for release builds, matching the layout of packaged binaries:

| Path                 | Purpose                                                                |
| -------------------- | ---------------------------------------------------------------------- |
| `config/config.toml` | Host configuration and generated local session secret                  |
| `config/dim.db`      | SQLite accounts, sessions, libraries, scan/index records, and progress |
| `metadata/`          | Durable downloaded artwork, avatars, and metadata                      |
| `streaming_cache/`   | Disposable playback and transcoding data                               |
| `logs/`              | Disposable runtime diagnostics                                         |

The historical `config/dim.db` filename and `dim_session` cookie name remain compatibility
boundaries: renaming either would make an existing installation appear empty or invalidate active
sessions. Eclipse reuses them directly. Legacy `DIM_*` environment variables remain fallback
aliases; new deployments should use the corresponding `ECLIPSE_*` names. If upgrading from a
version that exposed `secret_key`, remove that setting once and restart Eclipse to generate a new
secret and invalidate existing sessions.

Eclipse listens on `127.0.0.1` by default. Trusted LAN access is opt-in with an explicit listener,
for example `eclipse --bind-address 0.0.0.0`, `ECLIPSE_BIND_ADDRESS=0.0.0.0`, or
`bind_address = "0.0.0.0"` in the configuration file. The effective listener is logged at startup.

Direct internet exposure is unsupported. Eclipse has no built-in TLS termination; use an appropriately configured trusted reverse proxy when HTTPS access is intentional. See [the deployment and authentication boundary](docs/design/deployment-security.md) for proxy settings, session migration behavior, and security limits.

## Automated releases

`Cargo.toml`'s `workspace.package.version` is the single application-version source. Every Eclipse
workspace crate inherits it, and Cargo synchronizes `Cargo.lock` when the release command changes
the version.

The root release commands validate, version, commit, push, and tag the synchronized release branch.
The pushed tag starts `.github/workflows/release.yml`, which builds and publishes the Linux archive,
checksum, Eclipse container tags, and GitHub Release. The command waits for that exact tag and
commit's workflow run and succeeds only after confirming that the GitHub Release exists.

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
merges, rebases, force-pushes, or overwrites remote history. The release remote must resolve
directly to `madebyjordan/eclipse`; redirects from the former repository name are rejected. Set
`ECLIPSE_RELEASE_BRANCH` or `ECLIPSE_RELEASE_REMOTE` only when the intended release branch or
remote is intentionally different.

Before mutation it fetches tags, verifies remote ancestry and tag/Release availability, reports
every file that will enter the release commit, and runs the same frontend and Rust validation used
by the release workflow. A real release updates `Cargo.toml` and `Cargo.lock`, stages the current
non-ignored project changes, and creates one `chore: release vX.Y.Z` commit. It pushes `master`
without force and verifies the remote commit before creating an annotated tag on that exact commit
and pushing the tag separately. The one tag-triggered workflow builds the Linux x86_64 archive and
checksum, publishes versioned Linux x86_64 `ghcr.io/madebyjordan/eclipse` tags, and creates an
`Eclipse vX.Y.Z` GitHub Release. Archives are named `eclipse-vX.Y.Z-linux-x86_64.tar.gz` and contain
the `eclipse` executable. Dry runs
execute the complete inspection and validation path but never modify files or the index, create a
commit or tag, push, or publish anything.

If a tag exists but publication failed, do not recreate or move it. After correcting the workflow,
rerun it for the immutable tag, then verify the release:

```sh
gh workflow run .github/workflows/release.yml --repo madebyjordan/eclipse -f tag=v0.3.1
gh run watch --repo madebyjordan/eclipse
gh release view v0.3.1 --repo madebyjordan/eclipse
```

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

Download `eclipse-vX.Y.Z-linux-x86_64.tar.gz` and its `.sha256` file from the fork's GitHub Release,
then verify and unpack it:

1. Verify with `sha256sum -c eclipse-vX.Y.Z-linux-x86_64.tar.gz.sha256`.
2. Unpack with `tar -xvzf eclipse-vX.Y.Z-linux-x86_64.tar.gz`.
3. Run `cd release && ./eclipse`.
4. Access the Eclipse web UI at `http://localhost:8000`.

## Running with docker

The following command runs Eclipse on port 8000, storing configuration in `$HOME/.config/eclipse`.
You may change that path if you'd like to store configuration somewhere else.
You can mount as many directories containing media as you like by repeating the `-v HOST_PATH:CONTAINER_PATH` option.
In this example, the path `/media` on the host is made available at the same path inside the Docker container.
This name "media" is arbitrary and you can choose whatever you like.

```
docker run -d --name eclipse -p 127.0.0.1:8000:8000/tcp -e ECLIPSE_BIND_ADDRESS=0.0.0.0 -v $HOME/.config/eclipse:/opt/eclipse/config -v /media:/media:ro ghcr.io/madebyjordan/eclipse:dev
```

The multi-architecture image resides at `ghcr.io/madebyjordan/eclipse:master`.

To use hardware acceleration, mount the relevant device:

```
docker run -d --name eclipse -p 127.0.0.1:8000:8000/tcp -e ECLIPSE_BIND_ADDRESS=0.0.0.0 -v $HOME/.config/eclipse:/opt/eclipse/config -v /media:/media:ro --device=/dev/dri/renderD128 ghcr.io/madebyjordan/eclipse:dev
```

Existing containers that explicitly mount legacy `/opt/dim/{config,metadata,streaming_cache,logs}`
paths remain supported: the entrypoint detects those mounts and runs against the legacy persistent
layout. New deployments use `/opt/eclipse`. Refer to [docker-compose-template.yml](docker-compose-template.yml).

## License

Eclipse is licensed under the AGPLv3 license (see [LICENSE.md](LICENSE.md) or https://opensource.org/licenses/AGPL-3.0)

## Screenshots

![Login_Page](docs/design/login_page.png)
![Add_Library Modal](docs/design/add_library.png)
![Media_Page](docs/design/media_page.jpg)
