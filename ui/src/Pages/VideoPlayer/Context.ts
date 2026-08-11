import React, { createContext } from "react";

interface VideoPlayerContext {
  videoRef: React.MutableRefObject<HTMLVideoElement | null>;
  videoPlayer: React.MutableRefObject<HTMLDivElement | null>;
  overlay: HTMLDivElement | null;
  seekTo: (newTime: number) => void;
  player: dashjs.MediaPlayerClass;
  pendingVideoSwitch: React.MutableRefObject<{
    position: number;
    targetIndex: number;
    targetSetId: number;
    wasPaused: boolean;
  } | null>;
  changeVideoQuality: (trackIndex: number) => Promise<void>;
}

// Intentionally naming the variable the same as the type.
// See: https://github.com/typescript-eslint/typescript-eslint/issues/2585
// eslint-disable-next-line @typescript-eslint/no-redeclare
export const VideoPlayerContext = createContext<VideoPlayerContext | null>(
  null
);
