# Eclipse frontend foundation

Eclipse is a greenfield SvelteKit client. It shares Dim's Rust backend and domain contracts, but it
is the repository's sole frontend implementation.

## Runtime boundaries

- `src/lib/api/transport.ts` is the only HTTP transport. Generated contract types remain
  framework-neutral in `src/lib/api/generated.ts`.
- `src/lib/auth/session.svelte.ts` owns session bootstrap and the short-lived JavaScript token. The
  durable session is the backend's HttpOnly cookie.
- `src/lib/realtime/socket.svelte.ts` owns WebSocket authentication, typed event dispatch, connection
  state, and exponential reconnection. Server/API data is not stored there.
- Route and component state stays local. Playback timing writes directly to the playback route's
  output element instead of updating application-wide reactive state.
- `src/lib/playback/` is reachable only from `/play/[fileId]`. dash.js and JASSUB are dynamic imports
  from that route and are absent from the initial shell dependency graph.

SvelteKit runs with SSR disabled and `adapter-static` emits `eclipse/build/index.html` as the SPA
fallback. Rust embeds that directory, serves exact assets before the fallback, marks hashed
`_app/immutable` assets immutable, and keeps HTML non-cacheable.

The authenticated root route is the Eclipse browsing experience: real libraries and scanner status
feed a persistent header, while the selected library is rendered as one continuous, lazy-loading
media carousel. Selecting an item resolves its full metadata and playback files without leaving the
browsing context. Search uses the existing catalogue endpoint, and owners can create libraries
through the existing server-side directory browser.

## Backend adaptations

The stable HTTP and playback contracts were sufficient. Two narrow adaptations were made:

1. The static handler and build/release paths now consume `eclipse/build` and support SvelteKit asset
   paths and client-side deep links.
2. WebSocket upgrades may authenticate from the existing `dim_session` cookie. Explicit token
   authentication remains compatible. This closes the previous gap where HTTP auth survived a fresh
   browser session but realtime could not reconnect without a JavaScript-readable token.

The OpenAPI playback-track schema now includes the fields already returned by Rust and required by
track and subtitle selection. Both frontends' generated files are kept current during the transition.

## Playback proof

The proof uses the production playback planner, session creation, DASH MPD and segment endpoints,
track replacement, subtitle endpoints, and session termination. VTT is fetched with authentication
and attached as a blob track. ASS is fetched the same way and rendered through a dynamically loaded
JASSUB worker/WASM instance. All dash.js, JASSUB, blob URL, native media, remote AirPlay, and backend
session resources are disposed on route unmount.

Safari's `webkitShowPlaybackTargetPicker` is capability-detected. On supported browsers the proof
creates a separate `target=airplay` session and uses the backend's authenticated remote HLS resource;
unsupported browsers expose no active control.

The product currently omits Watchlist, cast, and content-rating presentation because Dim has no
persisted domain contract for those values. They must not be synthesized from design fixtures. The
metadata provider can fetch cast upstream, but scanner ingestion does not currently store or expose
it. The existing `rating` field is a provider score, not a content certification.
