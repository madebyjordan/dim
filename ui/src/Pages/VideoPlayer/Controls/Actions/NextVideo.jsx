import { useCallback, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useSelector } from "react-redux";
import { skipToken } from "@reduxjs/toolkit/query/react";
import NextVideoIcon from "../../../../assets/Icons/NextVideo";
import { UnfocusableButton } from "Components/unfocusableButton";
import { createPlaybackState } from "Pages/VideoPlayer/Navigation";

import {
  useGetMediaFilesQuery,
  useGetMediaQuery,
} from "../../../../api/v1/media";

function VideoActionNextVideo() {
  const video = useSelector((store) => store.video);

  const [enabled, setEnable] = useState(false);

  const location = useLocation();
  const navigate = useNavigate();
  const { data: currentMedia } = useGetMediaQuery(
    video.mediaID ? video.mediaID : skipToken
  );

  const nextEpisodeId = currentMedia.next_episode_id;

  const { data: nextMediaFiles } = useGetMediaFilesQuery(
    nextEpisodeId ? nextEpisodeId : skipToken
  );

  useEffect(() => {
    if (!nextEpisodeId) return;

    setEnable(true);
  }, [nextEpisodeId]);

  const nextVideo = useCallback(() => {
    const item = nextMediaFiles[0];
    if (!item) return;

    navigate(`/play/${item.id}`, {
      replace: true,
      state: createPlaybackState(location),
    });
  }, [location, navigate, nextMediaFiles]);

  return (
    <UnfocusableButton
      onClick={nextVideo}
      className={`next_video ${enabled}`}
      disabled={!enabled}
    >
      <NextVideoIcon />
    </UnfocusableButton>
  );
}

export default VideoActionNextVideo;
