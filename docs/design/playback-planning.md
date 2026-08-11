# Playback planning and transcoder lifecycle

Milestone 3 separates playback decisions from process creation. `dim-core` produces an inspectable
source/capability plan and stable public track IDs. The web boundary registers those plans without
calling FFmpeg. A manifest request is the admission boundary and materializes only its selected
video/audio IDs; subtitle extraction starts only when that subtitle is fetched.

The default policy is 8 active FFmpeg jobs globally, 4 per authenticated user, and 3 per playback
session. Sessions expire after 30 minutes without manifest, segment, subtitle, or control activity.
Deployments can make narrow operational overrides with `DIM_TRANSCODE_GLOBAL_LIMIT`,
`DIM_TRANSCODE_USER_LIMIT`, `DIM_TRANSCODE_SESSION_LIMIT`, and
`DIM_STREAM_SESSION_TTL_SECS`. These are intentionally not part of the broader runtime settings
redesign planned for Milestone 4.

The initial capability model is conservative and browser-oriented: H.264 and verified compatible
AV1 can use stream-copy DASH, while other video codecs transcode to H.264 and audio is normalized
to AAC. The plan response makes that choice inspectable. Manual quality changes replace the active
video recipe lazily: the requested recipe is admitted and prepared, dash.js reloads a manifest
containing that video and the current audio, and the UI changes its selected quality only after
dash.js reports the requested video adaptation as effective. The previous video recipe is retired
instead of accumulating processes, and playback position and paused state are restored after the
reload.

When Direct Play is available, it is the source-resolution option and source-height transcodes are
omitted as redundant; only lower, non-upscaled ladder entries are offered. Its label includes a
video bitrate only when ffprobe reports a stream-level value. Transcode labels use the configured
recipe resolution and bitrate.

Automatic bandwidth negotiation remains deferred. Planned qualities are separate adaptations and
separate lazy FFmpeg jobs, so exposing all of them as an adaptive ladder would both require a
different manifest model and spend admission, CPU, and memory on renditions the viewer may never
use. Device-specific codec profiles also remain deferred playback work.

Nightfall remains vendored only as a narrow FFmpeg/fMP4 compatibility adapter. Dim owns planning,
admission, ownership, TTL, and cleanup. Remaining replacement work is documented in
`vendor/nightfall/DIM_PATCHES.md`.
