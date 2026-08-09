import { useCallback, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useSelector } from "react-redux";
import { skipToken } from "@reduxjs/toolkit/query/react";
import PrevVideoIcon from "../../../../assets/Icons/PrevVideo";
import { UnfocusableButton } from "Components/unfocusableButton";
import { createPlaybackState } from "Pages/VideoPlayer/Navigation";

import {
  useGetMediaFilesQuery,
  useGetMediaQuery,
} from "../../../../api/v1/media";

function VideoActionPrevVideo() {
  const video = useSelector((store) => store.video);

  const [enabled, setEnable] = useState(false);

  const location = useLocation();
  const navigate = useNavigate();
  const { data: currentMedia } = useGetMediaQuery(
    video.mediaID ? video.mediaID : skipToken
  );

  const prevEpisodeId = currentMedia.prev_episode_id;
  const { data: nextMediaFiles } = useGetMediaFilesQuery(
    prevEpisodeId ? prevEpisodeId : skipToken
  );

  useEffect(() => {
    if (!prevEpisodeId) return;

    setEnable(true);
  }, [prevEpisodeId]);

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
      className={`prev_video ${enabled}`}
      disabled={!enabled}
    >
      <PrevVideoIcon />
    </UnfocusableButton>
  );
}

export default VideoActionPrevVideo;
