<h1 align="center">Dim</h1>

![Dashboard](docs/design/dashboard.jpg)
[![Discord](https://img.shields.io/discord/834495310332035123)](https://discord.gg/gBPyQ7NVah)

## 2026
Dim is a project with a lot of potential, I said this back in 2021-2022 when I was broughr onto the project as the sole designer at the time and now I am taking the opportunity to continue work on Dim, focusing on improving its foundation and overall user experience, making it more broadly accessible to a wider audience.

Dim is a self-hosted media manager. With minimal setup, Dim will organize and beautify your media collections for playback on localhost or a trusted home network.

## Running from source

Dim's supported source-development path is macOS or Linux. The repository pins Rust in `rust-toolchain.toml`, Node.js in `.nvmrc`, and Yarn in `ui/package.json`.

### Prerequisites

- Git
- Rust 1.93.1, installed automatically through [rustup](https://rustup.rs/)
- Node.js 24.19.0 LTS, with Corepack available
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

The bootstrap script invokes Corepack directly, so it does not need to install Yarn globally or enable Corepack's global shims.

### Build and run

From a fresh clone:

```sh
git clone https://github.com/Dusk-Labs/dim.git
cd dim
yarn build
yarn dev
```

Open [http://localhost:8000](http://localhost:8000) after Dim starts. To build and run an optimized binary, pass `--release` to both commands:

```sh
yarn build --release
yarn dev --release
```

The bootstrap script installs the locked frontend dependencies, builds the embedded web UI, links the system FFmpeg tools under `utils/`, and builds the Rust workspace. It never overwrites existing FFmpeg binaries in `utils/`.

Dim creates local state relative to its runtime directory. `scripts/run.sh` uses the repository root for development builds and `target/release/` for release builds, matching the layout of packaged binaries:

| Path | Purpose |
| --- | --- |
| `config/config.toml` | Host configuration and generated local session secret |
| `config/dim.db` | SQLite database |
| `metadata/` | Downloaded artwork and metadata |
| `streaming_cache/` | Temporary playback and transcoding data |

Existing configuration files are preserved. If upgrading from a version that exposed `secret_key`, remove that setting once and restart Dim to generate a new secret and invalidate existing sessions.

Dim listens on all local interfaces so trusted devices on the same home network can connect using the host machine's IP address. The current deployment model is not intended for direct internet exposure.

## Running from binaries

### Dependencies

* libva2
* libva-drm2
* libharfbuzz
* libfontconfig
* libfribidi
* libtheora
* libvorbis
* libvorbisenc
* libtheora0

You can then obtain binaries from the release tab in github:

1. Unpack with `unzip ./release-linux.zip && tar -xvzf ./release.tar.gz`
2. Run `cd release && ./dim`
3. Access the Dim web UI at `http://localhost:8000`.

## Running with docker

The following command runs dim on port 8000, storing configuration in `$HOME/.config/dim`.
You may change that path if you'd like to store configuration somewhere else.
You can mount as many directories containing media as you like by repeating the `-v HOST_PATH:CONTAINER_PATH` option.
In this example, the path `/media` on the host is made available at the same path inside the Docker container.
This name "media" is arbitrary and you can choose whatever you like.

```
docker run -d -p 8000:8000/tcp -v $HOME/.config/dim:/opt/dim/config -v /media:/media:ro ghcr.io/dusk-labs/dim:dev
```
The multi-architecture image resides at `ghcr.io/dusk-labs/dim:master`.

To use hardware acceleration, mount the relevant device:

```
docker run -d -p 8000:8000/tcp -v $HOME/.config/dim:/opt/dim/config -v /media:/media:ro --device=/dev/dri/renderD128 ghcr.io/dusk-labs/dim:dev
```

Refer to [docker-compose-template.yml](docker-compose-template.yml) to run Dim using Docker Compose.

## License

Dim is licensed under the AGPLv3 license (see [LICENSE.md](LICENSE.md) or https://opensource.org/licenses/AGPL-3.0)

## Screenshots

![Login_Page](docs/design/login_page.png)
![Add_Library Modal](docs/design/add_library.png)
![Media_Page](docs/design/media_page.jpg)
