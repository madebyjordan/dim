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

The initial capability model is conservative and browser-oriented: H.264 can use stream-copy DASH,
while other video codecs transcode to H.264 and audio is normalized to AAC. The plan response makes
that choice inspectable. Device-specific codec profiles, bandwidth negotiation, and seamless
mid-playback activation of an unstarted alternate rendition remain deferred playback-UX work.

Nightfall remains vendored only as a narrow FFmpeg/fMP4 compatibility adapter. Dim owns planning,
admission, ownership, TTL, and cleanup. Remaining replacement work is documented in
`vendor/nightfall/DIM_PATCHES.md`.
