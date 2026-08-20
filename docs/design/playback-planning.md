# Playback planning and transcoder lifecycle

Milestone 3 separates playback decisions from process creation. `dim-core` produces an inspectable
source/capability plan and stable public track IDs. The web boundary registers those plans without
calling FFmpeg. A manifest request is the admission boundary and materializes only its selected
video/audio IDs; subtitle extraction starts only when that subtitle is fetched.

The default policy is 8 active FFmpeg jobs globally, 4 per authenticated user, and 3 per playback
session. Sessions expire after 30 minutes without manifest, segment, subtitle, or control activity.
Deployments can make narrow operational overrides with `ECLIPSE_TRANSCODE_GLOBAL_LIMIT`,
`ECLIPSE_TRANSCODE_USER_LIMIT`, `ECLIPSE_TRANSCODE_SESSION_LIMIT`, and
`ECLIPSE_STREAM_SESSION_TTL_SECS`. The former `DIM_*` names remain fallback aliases for existing
deployments. These are intentionally not part of the broader runtime settings
redesign planned for Milestone 4.

Capability negotiation is source-specific and conservative. The server derives RFC 6381 codec
descriptors and exact video and audio configurations from ffprobe metadata. The browser checks each
configuration with `canPlayType`, MSE, and Media Capabilities, and returns structured evidence to
the planner. Preserving a source stream requires both a matching positive browser result and
independent server-side fMP4 remux eligibility; missing, stale, or inconclusive evidence falls back
to transcoding. No browser family, codec exception, or machine model is encoded in the policy.

The browser H.264 fallback is an explicit output contract: High profile, an RFC 6381 level derived
from the selected rendition, 8-bit 4:2:0, limited-range BT.709. PQ and HLG sources are converted to
linear light, tone mapped, converted to BT.709, and stripped of source HDR side data. Hardware
profiles are used only for verified 8-bit SDR inputs; sources needing unverified bit-depth or HDR
conversion fall through to the software profile. Audio that cannot be positively preserved is
normalized to AAC, including the established channel-layout normalization. The plan response
makes the choice inspectable. Manual quality changes replace the active video recipe lazily: the
requested recipe is admitted and prepared, dash.js reloads a manifest containing that video and
the current audio, and the UI changes its selected quality only after dash.js reports the requested
video adaptation as effective. The previous video recipe is retired instead of accumulating
processes, and playback position and paused state are restored after the reload.

When Direct Play is available, it is the source-resolution option and source-height transcodes are
omitted as redundant; only lower, non-upscaled ladder entries are offered. Its label includes a
video bitrate only when ffprobe reports a stream-level value. Transcode labels use the configured
recipe resolution and bitrate.

Automatic bandwidth negotiation remains deferred. Planned qualities are separate adaptations and
separate lazy FFmpeg jobs, so exposing all of them as an adaptive ladder would both require a
different manifest model and spend admission, CPU, and memory on renditions the viewer may never
use. Device-specific codec profiles also remain deferred playback work.

## AirPlay target

AirPlay is the first remote playback target. WebKit supplies route availability, the system target
picker, and wireless-route state on an `HTMLMediaElement`; it does not expose the selected
receiver's codec, HDR, resolution, or channel-layout capabilities. Local Safari codec queries are
therefore never treated as receiver evidence. Until stronger target evidence is available, the
planner takes the conservative Apple HLS compatibility path: H.264 High Profile SDR video and
stereo AAC-LC audio. The existing source-aware recipes still decide which source streams must be
changed and retain the existing source-bounded rendition ladder.

The browser creates a separate, lazy AirPlay playback session and gives WebKit a native HLS URL.
The URL contains a random token scoped to that playback session because the receiver fetches the
HLS resource itself and cannot send Grin's browser authorization header. Every playlist, init
segment, and media segment revalidates that token against the owning session. The multivariant and
media playlists reuse the existing fMP4 jobs; no second compatibility or transcoding pipeline is
introduced. Selecting a wireless route transfers play, pause, seek, progress, termination, and
error ownership to the AirPlay media element, and disconnecting restores the position to local
DASH playback.

Nightfall remains vendored only as a narrow FFmpeg/fMP4 compatibility adapter. Eclipse owns planning,
admission, ownership, TTL, and cleanup. Remaining replacement work is documented in
`vendor/nightfall/DIM_PATCHES.md`.
