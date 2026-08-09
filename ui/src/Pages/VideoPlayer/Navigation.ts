import type { Location, NavigateFunction } from "react-router";

export type PlaybackOriginState = { from: string };
type PlaybackLocation = Pick<Location, "pathname"> & {
  search?: string;
  hash?: string;
  state?: unknown;
};
type PlaybackContext = {
  mediaID?: number | string | null;
  libraryID?: number | string | null;
};

const isPlaybackPath = (path: string) => /^\/play(?:\/|$)/.test(path);

const getCurrentPath = (location: PlaybackLocation) =>
  `${location.pathname || ""}${location.search || ""}${location.hash || ""}`;

export const getPlaybackOrigin = (location?: PlaybackLocation) => {
  if (!location || typeof location.state !== "object" || !location.state) {
    return null;
  }

  const origin = (location.state as Partial<PlaybackOriginState>).from;

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

export const createPlaybackState = (
  location: PlaybackLocation
): PlaybackOriginState | undefined => {
  const origin = getPlaybackOrigin(location);
  if (origin) return { from: origin };

  const currentPath = getCurrentPath(location);
  if (!currentPath || isPlaybackPath(currentPath)) return undefined;

  return { from: currentPath };
};

export const navigateBackFromPlayback = (
  navigate: NavigateFunction,
  location: PlaybackLocation,
  { mediaID, libraryID }: PlaybackContext = {}
) => {
  if (getPlaybackOrigin(location)) {
    navigate(-1);
    return;
  }

  if (mediaID) {
    navigate(`/media/${mediaID}`, { replace: true });
    return;
  }

  if (libraryID) {
    navigate(`/library/${libraryID}`, { replace: true });
    return;
  }

  navigate("/", { replace: true });
};
