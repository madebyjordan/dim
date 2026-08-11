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
  scan_status?: "scanning" | "complete" | "failed";
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

export type ScanStatus = { status: "scanning" | "complete" | "failed" };

export type RematchMedia = { external_id: string; media_type: "movie" | "tv" };

export type ExternalMedia = Record<string, unknown>;

export type PlaybackTrack = {
  id: string;
  content_type: "video" | "audio" | "subtitle";
  set_id?: number;
  bandwidth?: number;
  height?: string;
  chunk_path?: string;
};

export type PlaybackSession = { gid: string; tracks: Array<PlaybackTrack> };

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
    | "EventAuthOk"
    | "EventAuthErr"
    | "MediafileMatched";
  id: number;
};

export interface ApiOperations {
  login: "/auth/login";
  register: "/auth/register";
  adminExists: "/auth/admin_exists";
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
  createPlaybackSession: "/stream/{id}/manifest";
  getPlaybackFailure: "/stream/{gid}/state/get_stderr";
  killPlaybackSession: "/stream/{gid}/state/kill";
}
