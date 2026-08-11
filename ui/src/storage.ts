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

type PlaybackSessionRemover = (gid: string) => Promise<unknown>;

const isMissingPlaybackSession = (error: unknown) =>
  typeof error === "object" && error !== null && "status" in error
    ? error.status === 404
    : false;

export const terminatePlaybackSession = async (
  gid: string,
  remove: PlaybackSessionRemover
) => {
  try {
    await remove(gid);
  } catch (error) {
    if (!isMissingPlaybackSession(error)) throw error;
  }

  if (getPlaybackSession() === gid) clearPlaybackSession();
};

export const reclaimPlaybackSession = async (
  remove: PlaybackSessionRemover
) => {
  const gid = getPlaybackSession();
  if (gid) await terminatePlaybackSession(gid, remove);
};
