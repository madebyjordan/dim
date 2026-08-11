export const PLAYBACK_ERROR_MESSAGE =
  "Dim could not prepare this video for playback. You can retry or return to the library.";

export const RETRY_POSITION_KEY = "dim:playback:retry-position";

export const consumeRetryPosition = (storage) => {
  const value = Number(storage.getItem(RETRY_POSITION_KEY));
  storage.removeItem(RETRY_POSITION_KEY);
  return Number.isFinite(value) && value > 0 ? value : null;
};

export const stopFailedPlayback = async ({
  details,
  gid,
  logger = console.error,
  request = fetch,
  token,
}) => {
  let ffmpegDiagnostics = [];

  if (gid) {
    try {
      const response = await request(`/api/v1/stream/${gid}/state/get_stderr`, {
        headers: { Authorization: token },
      });
      if (response.ok) {
        const payload = await response.json();
        ffmpegDiagnostics = payload.errors || [];
      }
    } catch (_) {}
  }

  logger("[VIDEO] playback failed", {
    details,
    ffmpegDiagnostics,
    gid,
  });

  if (gid) {
    try {
      await request(`/api/v1/stream/${gid}/state/kill`, {
        method: "DELETE",
        headers: { Authorization: token },
      });
    } catch (_) {}
  }

  return { msg: PLAYBACK_ERROR_MESSAGE };
};
