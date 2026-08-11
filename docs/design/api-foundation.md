# Milestone 5 API and frontend foundation

`api-contract/openapi.json` is the machine-readable authority for the actively migrated HTTP and
websocket surface. `yarn contract:generate` creates `ui/src/api/generated.ts`; generated code is
checked in so Rust-only and release builds do not need a TypeScript generator. `yarn
contract:check` fails when the artifact drifts.

The target frontend architecture is the existing RTK Query `v1` service backed by
`ui/src/api/transport.ts`. It owns the API base path, authentication header, same-origin cookie
policy, JSON/text parsing, abort propagation, timeout, normalized errors, offline detection, and
session-expiry notification. New domain requests belong in that service. `apiRequest` is the same
transport exposed for imperative integrations such as dash.js callbacks; it is not a second data
cache.

Migrated endpoints are account/auth contract shapes, library resources and scan state, media and
external rematch, progress, and playback session creation/failure/teardown. Login, registration,
password changes, account deletion, library media reads, rematch/search, progress, and playback
session creation/teardown now use the primary query architecture. Existing RTK Query media,
dashboard, search, file-browser and mediafile reads automatically use the centralized transport.

The websocket is owned by one `DimWebSocket` state machine. It authenticates only after `open`,
allows one live socket and one reconnect timer, uses capped exponential backoff with jitter,
probes server health after an idle interval, tears down timers and sockets on unmount, and parses
contract-typed events. The raw socket remains in context temporarily for existing event consumers.

## Remaining migration inventory

- Invite management, username/avatar changes, and settings screens still use legacy thunks. The
  auth reducer remains the compatibility authority for the token while legacy consumers remain;
  delete its superseded request-state branches once those consumers have moved.
- Library creation/deletion/scan actions still feed existing modal/sidebar reducers. The RTK Query
  endpoints are ready, but those consumers must move together to avoid duplicate loading state.
- Playback manifest/progress/subtitle calls inside dash.js and player event adapters remain
  imperative. Route them through `apiRequest` or the prepared RTK mutations when each player
  callback is converted to strict TypeScript.
- Several card, modal, player, preferences, and legacy reducer modules remain JavaScript. Convert
  them by domain while removing their superseded state, not mechanically.
- Existing websocket listeners still parse their own domain payloads from the raw context socket.
  Replace them with typed event subscriptions as each domain migrates, then make the raw socket
  private to the manager.
