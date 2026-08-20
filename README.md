<h1 align="center">Eclipse</h1>

<p align="center">
  A self-hosted personal media manager with a Rust backend and a SvelteKit web interface.
</p>

<p align="center">
  <a href="https://discord.gg/gBPyQ7NVah"><img alt="Discord" src="https://img.shields.io/discord/834495310332035123"></a>
  <a href="LICENSE.md"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg"></a>
</p>

![Eclipse dashboard](docs/design/eclipse-design-home.png)

Eclipse is a spiritual successor to Dim - it's a self hosted media manager that organizes your personal media library through a browser. Eclipse retains Dim's Rust crate structure and Nightfall media engine and will actively continue to expand on the overall user experience.

## Quick start installation

Clone the repository, then run the guided setup for your platform:

```sh
git clone https://github.com/madebyjordan/eclipse.git
cd eclipse
```

| Platform                  | Command        |
| ------------------------- | -------------- |
| Windows PowerShell or CMD | `install.cmd`  |
| macOS or Linux            | `./install.sh` |

The installer checks the required toolchain, offers supported dependency recovery, builds an
optimized release, and can open Eclipse at [http://localhost:8000](http://localhost:8000). WSL is
not required on Windows.

Already have the prerequisites? Jump to [Development](#development).

## Development

### Prerequisites

The repository pins its primary development tools:

- Git
- Rust 1.93.1 through `rust-toolchain.toml`
- Node.js 24.19.0 through `.nvmrc`
- pnpm 11.9.0 through `package.json`
- FFmpeg and FFprobe 9.0 or newer (both executables are checked at build and runtime)
- SQLite development tools
- A C/C++ build toolchain and `pkg-config`
- OpenSSL development headers on Linux
- `libva-dev`, `libva-drm2`, and `libva2` for Linux VAAPI builds

The guided installer can prepare these requirements. For manual setup, enable the pinned package
manager after installing Node.js:

```sh
corepack enable pnpm
```

Common native packages:

```sh
# macOS
brew install ffmpeg sqlite pkg-config

# Debian or Ubuntu
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev sqlite3 ffmpeg
```

Eclipse requires the installed `ffmpeg` and `ffprobe` commands to report major version 9 or
newer. Homebrew's current `ffmpeg`/`ffmpeg@9` formula satisfies that baseline. On Linux, use a
distribution repository that provides FFmpeg 9; the installer fails with a version diagnostic
instead of accepting an older distro package. Release and container builds use a pinned,
checksum-verified FFmpeg 9.0.1 static toolchain for x86_64 and arm64.

On Windows, the installer uses WinGet where appropriate and configures a user-level pnpm shim for
new PowerShell, CMD, and Git Bash sessions. **Visual Studio Build Tools must include the MSVC x64 compiler and a Windows SDK**.

### Build and run

The universal developer commands are:

```sh
pnpm build
pnpm dev
```

Open [http://localhost:5173](http://localhost:5173) for development. Vite provides hot module
replacement and proxies API, image, playback, and WebSocket traffic to the Rust backend on port 8000.

For an optimized single-server build:

```sh
pnpm build --release
pnpm dev --release
```

The release server is available at [http://localhost:8000](http://localhost:8000).

### Useful commands

| Command                 | Purpose                                                                     |
| ----------------------- | --------------------------------------------------------------------------- |
| `pnpm build`            | Install locked frontend dependencies and build the UI and debug Rust binary |
| `pnpm build --release`  | Build the optimized Rust binary and release runtime                         |
| `pnpm dev`              | Rebuild and run the Rust backend with the Vite development server           |
| `pnpm dev --release`    | Run the optimized backend with the embedded frontend                        |
| `pnpm test`             | Run the project, frontend, and locked Rust test suites                      |
| `pnpm test:build`       | Run build-orchestration regression tests                                    |
| `pnpm release:validate` | Run the complete release validation gate                                    |

The cross-platform build entrypoint is `scripts/build.mjs`. The shell scripts in `scripts/` remain
thin compatibility wrappers for the installer and existing Unix automation. Existing FFmpeg and
FFprobe tools under `utils/` are preserved.

## Installer options

The guided installer detects an existing runtime before building:

- **Reinstall/update** preserves configuration and application data.
- **Reset** removes host settings, cache, and logs while preserving the library and user data.
- **Clean install** removes managed state after explicit confirmation.

Useful non-interactive options include:

```sh
./install.sh --platform linux --yes --no-start
./install.sh --existing-action reinstall --yes --no-start
./install.sh --demo --demo-scenario fresh
```

Use `install.cmd` instead of `./install.sh` from Windows PowerShell or CMD. Supported platforms are
`macos`, `linux`, and `windows`; existing-install actions are `reinstall`, `reset`, `clean`, and
`exit`. Demo mode does not inspect or modify the system.

## Runtime data and security

Eclipse stores state relative to its runtime directory: the repository root for debug development
and `target/release/` for optimized local builds.

| Path                 | Contents                                                |
| -------------------- | ------------------------------------------------------- |
| `config/config.toml` | Host settings and generated session secret              |
| `config/dim.db`      | Accounts, sessions, libraries, progress, and scan state |
| `metadata/`          | Downloaded artwork, avatars, and metadata               |
| `streaming_cache/`   | Disposable transcoding and playback data                |
| `logs/`              | Runtime diagnostics                                     |

The historical `config/dim.db` filename and `dim_session` cookie name are retained for
compatibility. Legacy `DIM_*` environment variables remain fallback aliases; new deployments
should use the corresponding `ECLIPSE_*` variables.

Eclipse listens on `127.0.0.1` by default. Trusted LAN access is opt-in:

```sh
eclipse --bind-address 0.0.0.0
```

The equivalent environment variable is `ECLIPSE_BIND_ADDRESS=0.0.0.0`, and the setting can also be
stored as `bind_address = "0.0.0.0"` in `config.toml`.

Direct internet exposure is unsupported. Eclipse does not terminate TLS; use a trusted reverse
proxy for intentional HTTPS access. See the
[deployment and authentication boundary](docs/design/deployment-security.md) for proxy settings
and security constraints.

## Deployment

### Docker

The `dev` image tracks the current `master` branch. Published releases also receive versioned
container tags.

```sh
docker run -d \
  --name eclipse \
  --restart unless-stopped \
  -p 127.0.0.1:8000:8000 \
  -e ECLIPSE_BIND_ADDRESS=0.0.0.0 \
  -v eclipse-config:/opt/eclipse/config \
  -v eclipse-metadata:/opt/eclipse/metadata \
  -v eclipse-cache:/opt/eclipse/streaming_cache \
  -v eclipse-logs:/opt/eclipse/logs \
  -v /path/to/media:/media:ro \
  ghcr.io/madebyjordan/eclipse:dev
```

Add more read-only media mounts as needed. For Linux VAAPI acceleration, also pass the render
device:

```sh
--device /dev/dri/renderD128:/dev/dri/renderD128
```

New containers use `/opt/eclipse`. Existing deployments that mount legacy
`/opt/dim/{config,metadata,streaming_cache,logs}` paths remain supported.

### Linux release archive

GitHub Releases provide a Linux x86_64 archive and SHA-256 checksum. Replace `vX.Y.Z` with the
release you downloaded:

```sh
sha256sum -c eclipse-vX.Y.Z-linux-x86_64.tar.gz.sha256
tar -xzf eclipse-vX.Y.Z-linux-x86_64.tar.gz
cd release
./eclipse
```

The binary uses the host's VAAPI, font, Theora, and Vorbis runtime libraries. Package names vary by
distribution; the Docker image is the simplest option when those libraries are unavailable.

## Releases for maintainers

`Cargo.toml`'s `workspace.package.version` is the application version source. Release commands
validate the workspace, update the version and lockfile, create the release commit and immutable
tag, push without force, and wait for the matching GitHub Actions release.

Always inspect a dry run first:

```sh
pnpm release:patch -- --dry-run
pnpm release:patch

# Or choose a different semantic-version bump:
pnpm release:minor -- --dry-run
pnpm release:minor
pnpm release:major -- --dry-run
pnpm release:major
```

Run releases from `master` tracking `origin/master`, with authenticated `gh` and Git access. The
release workflow publishes the Linux x86_64 archive, checksum, versioned container image, and
GitHub Release. It never pulls, rebases, force-pushes, or moves an existing tag.

If publication for an immutable tag must be retried:

```sh
gh workflow run .github/workflows/release.yml --repo madebyjordan/eclipse -f tag=vX.Y.Z
gh run watch --repo madebyjordan/eclipse
gh release view vX.Y.Z --repo madebyjordan/eclipse
```

## Screenshots

<details>
<summary>View more screenshots</summary>

![Eclipse login page](docs/design/login_page.png)

![Add library dialog](docs/design/add_library.png)

![Eclipse media page](docs/design/media_page.jpg)

</details>

## Community and license

Questions and project discussion are welcome on [Discord](https://discord.gg/gBPyQ7NVah).

Eclipse is licensed under the [GNU Affero General Public License v3.0](LICENSE.md).
