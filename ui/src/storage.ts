export const storageKeys = {
  playbackSession: "dim:playback:session",
  playbackRetryPosition: "dim:playback:retry-position",
} as const;

const legacyPlaybackSessionKey = "GID";

export const getPlaybackSession = () => {
  const current = sessionStorage.getItem(storageKeys.playbackSession);
  if (current) return current;
  const legacy = sessionStorage.getItem(legacyPlaybackSessionKey);
  if (legacy) {
    sessionStorage.setItem(storageKeys.playbackSession, legacy);
    sessionStorage.removeItem(legacyPlaybackSessionKey);
  }
  return legacy;
};

export const setPlaybackSession = (gid: string) => {
  sessionStorage.setItem(storageKeys.playbackSession, gid);
};

export const clearPlaybackSession = () => {
  sessionStorage.removeItem(storageKeys.playbackSession);
  sessionStorage.removeItem(legacyPlaybackSessionKey);
};
