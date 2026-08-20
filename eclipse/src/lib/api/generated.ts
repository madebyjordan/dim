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
  media_type: 'movie' | 'tv';
};

export type CreateLibraryResponse = { id: number; scan_status: 'scanning' };

export type Library = {
  id: number;
  name: string;
  locations: Array<string>;
  media_type: string;
  auto_scan: boolean;
  scan_status?: 'scanning' | 'complete' | 'failed' | 'cancelled';
};

export type UpdateLibrary = { auto_scan: boolean };

export type Chapters = { credits: number };

export type MediaSummary = {
  id: number;
  name: string;
  poster_path?: string | null;
};

export type SearchResult = {
  id: number;
  library_id: number;
  name: string;
  poster_path?: string | null;
};

export type Media = {
  id: number;
  name: string;
  library_id: number;
  media_type: string;
  genres: Array<string>;
  duration: number;
  progress?: number;
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
  season_count?: number;
  start_year?: number | null;
  end_year?: number | null;
  ongoing?: boolean;
  year?: number;
  tags?: Record<string, unknown>;
};

export type MediaFile = {
  id: number;
  media_id?: number | null;
  library_id: number;
  target_file: string;
  raw_name: string;
  raw_year?: number | null;
  quality?: string | null;
  codec?: string | null;
  container?: string | null;
  audio?: string | null;
  original_resolution?: string | null;
  duration?: number | null;
  episode?: number | null;
  season?: number | null;
  corrupt?: boolean | null;
  channels?: number | null;
  profile?: string | null;
  audio_language?: string | null;
  manual_override: boolean;
};

export type DirectoryEntry = { name: string; path: string };

export type DirectoryListing = {
  current: string;
  parent: string | null;
  directories: Array<DirectoryEntry>;
};

export type StorageRoot = {
  display_name: string;
  path: string;
  available_bytes: number;
  kind: 'fixed' | 'removable' | 'network';
};

export type UnmatchedMediaFile = {
  id?: number;
  name?: string;
  folder?: string;
  duration?: number;
  file?: string;
  files?: Array<UnmatchedMediaFile>;
  type: 'directory' | 'file';
};

export type UnmatchedFiles = {
  count: number;
  files: Array<UnmatchedMediaFile>;
};

export type ScanStatus = {
  status: 'scanning' | 'complete' | 'failed' | 'cancelled';
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

export type RematchMedia = { external_id: string; media_type: 'movie' | 'tv' };

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
  probe_source: 'ingestion' | 'fallback';
};

export type PlaybackTrack = {
  id: string;
  content_type: 'video' | 'audio' | 'subtitle';
  set_id: number;
  is_direct: boolean;
  mime: string;
  codecs: string;
  bandwidth: number;
  average_bandwidth: number;
  height?: string;
  duration?: number | null;
  chunk_path: string;
  init_seg?: string | null;
  is_default: boolean;
  label: string;
  lang?: string | null;
  target_duration: number;
  audio_channels?: number | null;
  frame_rate?: number | null;
  video_range?: string | null;
};

export type AudioPlaybackPlan = {
  source: Record<string, unknown>;
  reported_capability: Record<string, unknown> | null;
  chosen_action: 'preserve' | 'transcode_aac';
  decision_reason: string;
};

export type PlaybackPlan = {
  target: 'browser' | 'airplay';
  capability_evidence: string;
  preferred_strategy: 'direct_play' | 'transcode';
  direct_play_supported: boolean;
  decision_reason: string;
  renditions: Array<{ height: number; bitrate: number }>;
  audio: Array<AudioPlaybackPlan>;
};

export type RemotePlaybackResource = { kind: 'airplay'; url: string };

export type PlaybackSession = {
  gid: string;
  tracks: Array<PlaybackTrack>;
  playback_plan: PlaybackPlan;
  remote?: RemotePlaybackResource | null;
};

export type WebSocketAuthenticate = { type: 'authenticate'; token: string };

export type WebSocketEvent = {
  type:
    | 'EventNewCard'
    | 'EventRemoveCard'
    | 'EventNewLibrary'
    | 'EventRemoveLibrary'
    | 'EventStreamIsReady'
    | 'EventStreamStats'
    | 'EventStartedScanning'
    | 'EventStoppedScanning'
    | 'EventScanFailed'
    | 'EventScanCancelled'
    | 'EventAuthOk'
    | 'EventAuthErr'
    | 'MediafileMatched';
  id: number;
};

export interface ApiOperations {
  login: '/auth/login';
  register: '/auth/register';
  adminExists: '/auth/admin_exists';
  logout: '/auth/logout';
  whoAmI: '/auth/whoami';
  changePassword: '/user/password';
  deleteAccount: '/user';
  listLibraries: '/library';
  createLibrary: '/library';
  getLibrary: '/library/{id}';
  updateLibrary: '/library/{id}';
  deleteLibrary: '/library/{id}';
  getLibraryMedia: '/library/{id}/media';
  getUnmatched: '/library/{id}/unmatched';
  getLibraryScan: '/library/{id}/scan';
  retryLibraryScan: '/library/{id}/scan';
  getMedia: '/media/{id}';
  getMediaFiles: '/media/{id}/files';
  rematchMedia: '/media/{id}/rematch';
  saveProgress: '/media/{id}/progress';
  searchMedia: '/search';
  listDirectories: '/filebrowser';
  listStorageRoots: '/filebrowser/roots';
  searchExternalMedia: '/media/tmdb_search';
  inspectPlaybackCapabilities: '/stream/{id}/capabilities';
  createPlaybackSession: '/stream/{id}/manifest';
  getPlaybackFailure: '/stream/{gid}/state/get_stderr';
  killPlaybackSession: '/stream/{gid}/state/kill';
}
