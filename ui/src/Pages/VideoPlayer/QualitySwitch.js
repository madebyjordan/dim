export const buildPlaybackManifestUrl = ({
  audioId,
  gid,
  replaceVideo = false,
  videoId,
}) => {
  const params = new URLSearchParams({
    start_num: "0",
    should_kill: "false",
    includes: [videoId, audioId].filter(Boolean).join(","),
  });
  if (replaceVideo) params.set("replace_video", "true");
  return `/api/v1/stream/${gid}/manifest.mpd?${params}`;
};

export const effectiveTrackIndex = (plannedTracks, playerTrack) => {
  if (!playerTrack) return -1;
  return plannedTracks.findIndex(
    (track) => String(track.set_id) === String(playerTrack.id)
  );
};
