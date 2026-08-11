import { useCallback, useEffect, useRef, useContext } from "react";
import { shallowEqual, useDispatch, useSelector } from "react-redux";

import SeekingTo from "./SeekingTo";
import { VideoPlayerContext } from "../Context";

import "./SeekBar.scss";
import { updateVideo } from "../../../actions/video";
import { useSaveProgressMutation } from "../../../api/v1/foundation";

function VideoSeekBar(props) {
  const dispatch = useDispatch();
  const [saveProgress] = useSaveProgressMutation();

  const { playbackController } = useContext(VideoPlayerContext);

  const video = useSelector((store) => store.video, shallowEqual);

  const seekBar = useRef(null);

  const seekBarCurrent = useRef(null);
  const bufferBar = useRef(null);

  const { seekTo } = props;
  // save progress every 15 seconds
  useEffect(() => {
    if (video.currentTime % 15 !== 0 || video.currentTime === 0) return;

    saveProgress({
      id: video.episode?.id || video.mediaID,
      offset: video.currentTime,
    });
  }, [saveProgress, video.currentTime, video.episode?.id, video.mediaID]);

  // current time
  useEffect(() => {
    const position = (video.currentTime / video.duration) * 100;
    seekBarCurrent.current.style.width = `${position}%`;
  }, [video.currentTime, video.duration]);

  // buffer
  useEffect(() => {
    const position =
      ((video.currentTime + video.buffer) / video.duration) * 100;
    bufferBar.current.style.width = `${position}%`;
  }, [video.currentTime, video.duration, video.buffer]);

  const onSeek = useCallback(
    async (e) => {
      if (video.seeking) return;

      dispatch(
        updateVideo({
          seeking: true,
        })
      );

      const rect = e.target.getBoundingClientRect();
      const percent = (e.clientX - rect.left) / rect.width;
      const videoDuration = playbackController.duration();
      const newTime = Math.floor(percent * videoDuration);

      seekTo(newTime);
    },
    [dispatch, playbackController, seekTo, video.seeking]
  );

  return (
    <div className="seekBarContainer">
      <div className="seekBar" onClick={onSeek} ref={seekBar}>
        <div ref={bufferBar} className="buffer" />
        <div ref={seekBarCurrent} className="current" />
      </div>
      <SeekingTo
        nameRef={props.nameRef}
        timeRef={props.timeRef}
        seekBar={seekBar}
      />
    </div>
  );
}

export default VideoSeekBar;
