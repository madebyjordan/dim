const isPlaybackPath = (path) => /^\/play(?:\/|$)/.test(path);

const getCurrentPath = (location) =>
  `${location.pathname || ""}${location.search || ""}${location.hash || ""}`;

export const getPlaybackOrigin = (location) => {
  const origin = location?.state?.from;

  if (
    typeof origin !== "string" ||
    !origin.startsWith("/") ||
    origin.startsWith("//") ||
    isPlaybackPath(origin)
  ) {
    return null;
  }

  return origin;
};

export const createPlaybackState = (location) => {
  const origin = getPlaybackOrigin(location);
  if (origin) return { from: origin };

  const currentPath = getCurrentPath(location);
  if (!currentPath || isPlaybackPath(currentPath)) return undefined;

  return { from: currentPath };
};

export const navigateBackFromPlayback = (
  history,
  { mediaID, libraryID } = {}
) => {
  if (getPlaybackOrigin(history.location)) {
    history.goBack();
    return;
  }

  if (mediaID) {
    history.replace(`/media/${mediaID}`);
    return;
  }

  if (libraryID) {
    history.replace(`/library/${libraryID}`);
    return;
  }

  history.replace("/");
};
