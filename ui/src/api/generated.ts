// Generated from api-contract/openapi.json. Do not edit.

export type ApiErrorEnvelope = { error: ApiError; request_id: string };

export type ApiError = {
  code: string;
  message: string;
  details?: Record<string, unknown>;
};

export type LoginRequest = {
  username: string;
  password: string;
  invite_token?: string | null;
};

export type LoginResponse = { token: string };

export type RegisterResponse = { token: string; username: string };

export type AdminExists = { exists: boolean };

export type User = {
  username: string;
  roles: Array<string>;
  picture?: string | null;
  spentWatching: number;
};

export type ChangePassword = { old_password: string; new_password: string };

export type DeleteAccount = { password: string };

export type CreateLibrary = {
  name: string;
  locations: Array<string>;
  media_type: "movie" | "tv";
};

export type Library = {
  id: number;
  name: string;
  locations: Array<string>;
  media_type: string;
  scan_status?: "scanning" | "complete" | "failed" | "cancelled";
};

export type Chapters = { credits: number };

export type Media = {
  id: number;
  name: string;
  library_id: number;
  media_type: string;
  genres: Array<string>;
  duration: number;
  progress: number;
  description?: string | null;
  added?: string | null;
  backdrop_path?: string | null;
  poster_path?: string | null;
  chapters?: Chapters;
  episode?: number;
  next_episode_id?: number;
  play_btn_id?: number;
  prev_episode_id?: number;
  rating?: number;
  season?: number;
  year?: number;
  tags?: Record<string, unknown>;
};

export type UnmatchedMediaFile = {
  id?: number;
  name?: string;
  folder?: string;
  duration?: number;
  file?: string;
  files?: Array<UnmatchedMediaFile>;
  type: "directory" | "file";
};

export type UnmatchedFiles = {
  count: number;
  files: Array<UnmatchedMediaFile>;
};

export type ScanStatus = {
  status: "scanning" | "complete" | "failed" | "cancelled";
  stage?: string;
  discovered?: number;
  processed?: number;
  committed?: number;
  skipped?: number;
  failed?: number;
  requested_at?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
  last_progress_at?: string | null;
  elapsed_seconds?: number;
  seconds_since_progress?: number | null;
  error_summary?: string | null;
};

export type RematchMedia = { external_id: string; media_type: "movie" | "tv" };

export type ExternalMedia = Record<string, unknown>;

export type VideoCapabilityRequest = {
  content_type: string;
  codec: string;
  codec_descriptor: string;
  width: number;
  height: number;
  bitrate: number;
  frame_rate: number;
  hdr: boolean;
  hdr_metadata_type?: string;
  color_gamut?: string;
  transfer_function?: string;
};

export type AudioCapabilityRequest = {
  stream_index: number;
  content_type: string;
  codec: string;
  codec_descriptor: string;
  channels: number;
  bitrate: number;
  sample_rate: number;
};

export type PlaybackCapabilityInspection = {
  video: {
    content_type?: string;
    codec?: string;
    codec_descriptor?: string;
    width?: number;
    height?: number;
    bitrate?: number;
    frame_rate?: number;
    hdr?: boolean;
    hdr_metadata_type?: string;
    color_gamut?: string;
    transfer_function?: string;
  } | null;
  audio: Array<AudioCapabilityRequest>;
  server_remux_supported: boolean;
  probe_source: "ingestion" | "fallback";
};

export type PlaybackTrack = {
  id: string;
  content_type: "video" | "audio" | "subtitle";
  set_id?: number;
  bandwidth?: number;
  height?: string;
  chunk_path?: string;
};

export type AudioPlaybackPlan = {
  source: Record<string, unknown>;
  reported_capability: Record<string, unknown> | null;
  chosen_action: "preserve" | "transcode_aac";
  decision_reason: string;
};

export type PlaybackPlan = {
  target: "browser" | "airplay";
  capability_evidence: string;
  preferred_strategy: "direct_play" | "transcode";
  direct_play_supported: boolean;
  decision_reason: string;
  renditions: Array<{ height: number; bitrate: number }>;
  audio: Array<AudioPlaybackPlan>;
};

export type RemotePlaybackResource = { kind: "airplay"; url: string };

export type PlaybackSession = {
  gid: string;
  tracks: Array<PlaybackTrack>;
  playback_plan: PlaybackPlan;
  remote?: RemotePlaybackResource | null;
};

export type WebSocketAuthenticate = { type: "authenticate"; token: string };

export type WebSocketEvent = {
  type:
    | "EventNewCard"
    | "EventRemoveCard"
    | "EventNewLibrary"
    | "EventRemoveLibrary"
    | "EventStreamIsReady"
    | "EventStreamStats"
    | "EventStartedScanning"
    | "EventStoppedScanning"
    | "EventScanFailed"
    | "EventScanCancelled"
    | "EventAuthOk"
    | "EventAuthErr"
    | "MediafileMatched";
  id: number;
};

export interface ApiOperations {
  login: "/auth/login";
  register: "/auth/register";
  adminExists: "/auth/admin_exists";
  logout: "/auth/logout";
  whoAmI: "/auth/whoami";
  changePassword: "/user/password";
  deleteAccount: "/user";
  listLibraries: "/library";
  createLibrary: "/library";
  getLibrary: "/library/{id}";
  deleteLibrary: "/library/{id}";
  getLibraryMedia: "/library/{id}/media";
  getUnmatched: "/library/{id}/unmatched";
  getLibraryScan: "/library/{id}/scan";
  retryLibraryScan: "/library/{id}/scan";
  getMedia: "/media/{id}";
  rematchMedia: "/media/{id}/rematch";
  saveProgress: "/media/{id}/progress";
  searchExternalMedia: "/media/tmdb_search";
  inspectPlaybackCapabilities: "/stream/{id}/capabilities";
  createPlaybackSession: "/stream/{id}/manifest";
  getPlaybackFailure: "/stream/{gid}/state/get_stderr";
  killPlaybackSession: "/stream/{gid}/state/kill";
}
