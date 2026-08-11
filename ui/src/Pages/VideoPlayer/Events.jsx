import { useCallback, useEffect, useContext, useRef } from "react";
import { shallowEqual, useDispatch, useSelector } from "react-redux";
import { MediaPlayer } from "dashjs";

import { VideoPlayerContext } from "./Context";
import { consumeRetryPosition, stopFailedPlayback } from "./PlaybackFailure";
import { effectiveTrackIndex } from "./QualitySwitch";

import {
  setManifestState,
  updateTrack,
  updateVideo,
} from "../../actions/video";

function VideoEvents() {
  const dispatch = useDispatch();
  const { pendingVideoSwitch, player, videoRef } =
    useContext(VideoPlayerContext);
  const failureHandled = useRef(false);

  const { token, video } = useSelector(
    (store) => ({
      token: store.auth.token,
      video: store.video,
    }),
    shallowEqual
  );

  const eManifestLoad = useCallback(() => {
    console.log("[VIDEO] manifest loaded");

    dispatch(
      setManifestState({
        loading: false,
        loaded: true,
      })
    );
  }, [dispatch]);

  const confirmVideoSwitch = useCallback(() => {
    const pending = pendingVideoSwitch.current;
    if (!pending) return false;
    const effectiveIndex = effectiveTrackIndex(
      video.tracks.video.list,
      player.getCurrentTrackFor("video")
    );
    if (
      effectiveIndex !== pending.targetIndex ||
      String(video.tracks.video.list[effectiveIndex]?.set_id) !==
        String(pending.targetSetId)
    ) {
      return false;
    }

    pendingVideoSwitch.current = null;
    dispatch(updateTrack("video", { current: effectiveIndex }));
    dispatch(
      updateVideo({
        currentTime: videoRef.current?.currentTime || pending.position,
        waiting: false,
      })
    );
    if (pending.wasPaused) videoRef.current?.pause();
    return true;
  }, [dispatch, pendingVideoSwitch, player, video.tracks.video.list, videoRef]);

  const eCanPlay = useCallback(() => {
    console.log("[VIDEO] can play");

    // we need to do all this shit so that the UI selects the correct tracks.
    const effectiveVideoIndex = effectiveTrackIndex(
      video.tracks.video.list,
      player.getCurrentTrackFor("video")
    );

    const audioTrack = player.getCurrentTrackFor("audio");
    const audioTrackIdx = video.tracks.audio.list.filter(
      (track) => track.set_id === audioTrack.id
    );

    const pending = pendingVideoSwitch.current;
    if (!pending && effectiveVideoIndex >= 0) {
      dispatch(updateTrack("video", { current: effectiveVideoIndex }));
    }

    dispatch(
      updateTrack("audio", {
        current: video.tracks.audio.list.indexOf(audioTrackIdx[0]),
      })
    );

    const retryPosition = consumeRetryPosition(sessionStorage);
    const restoredPosition = pending?.position ?? retryPosition;
    if (restoredPosition !== null && restoredPosition > 0.25) {
      if (pending?.wasPaused) videoRef.current?.pause();
      player.seek(restoredPosition);
    } else if (pending) {
      confirmVideoSwitch();
    }

    dispatch(
      updateVideo({
        canPlay: true,
        waiting: pending !== null,
        duration: Math.round(player.duration()) | 0,
        ...(restoredPosition !== null && { currentTime: restoredPosition }),
      })
    );
  }, [
    confirmVideoSwitch,
    dispatch,
    pendingVideoSwitch,
    player,
    video,
    videoRef,
  ]);

  const ePlaybackSeeked = useCallback(() => {
    confirmVideoSwitch();
  }, [confirmVideoSwitch]);

  const ePlayBackPaused = useCallback(() => {
    console.log("[VIDEO] paused");

    dispatch(
      updateVideo({
        paused: true,
      })
    );
  }, [dispatch]);

  const ePlayBackPlaying = useCallback(() => {
    dispatch(
      updateVideo({
        paused: false,
      })
    );
  }, [dispatch]);

  const ePlayBackWaiting = useCallback(() => {
    console.log("[VIDEO] playback waiting");

    dispatch(
      updateVideo({
        waiting: true,
      })
    );
  }, [dispatch]);

  const ePlayBackEnded = useCallback(() => {
    console.log("[VIDEO] playback ended");

    dispatch(
      updateVideo({
        playback_ended: true,
      })
    );
  }, [dispatch]);

  const eError = useCallback(
    (e) => {
      if (failureHandled.current) return;
      failureHandled.current = true;
      pendingVideoSwitch.current = null;

      (async () => {
        const error = await stopFailedPlayback({
          details: e?.error?.message || e?.event?.message || "Unknown error",
          gid: video.gid,
          token,
        });

        dispatch(
          setManifestState({
            loading: false,
            loaded: false,
          })
        );
        dispatch(
          updateVideo({
            canPlay: false,
            error,
            waiting: false,
          })
        );
      })();
    },
    [dispatch, pendingVideoSwitch, token, video.gid]
  );

  const ePlayBackNotAllowed = useCallback(
    (e) => {
      console.log("[VIDEO] playback not allowed");

      if (e.type === "playbackNotAllowed") {
        dispatch(
          updateVideo({
            paused: true,
          })
        );
      }
    },
    [dispatch]
  );

  /*
    PLAYBACK_PROGRESS event stops after error occurs
    so using this event from now on to get buffer length
  */
  const ePlayBackTimeUpdated = useCallback(
    (e) => {
      /*
      on some browsers (*cough*, chrome) current
      time gets reset back to 0 on seek
    */
      let newTime = Math.floor(e.time);

      if (newTime < video.prevSeekTo) {
        newTime += video.prevSeekTo - newTime;
      }

      dispatch(
        updateVideo({
          currentTime: newTime,
          buffer: Math.round(player.getBufferLength()),
          waiting: pendingVideoSwitch.current !== null,
        })
      );
    },
    [dispatch, pendingVideoSwitch, player, video.prevSeekTo]
  );

  const eQualityChange = useCallback(
    (e) => {
      console.log("[video] quality changing ", e);

      if (e.mediaType !== "video") return;
      if (pendingVideoSwitch.current) return;

      const tracks =
        e.mediaType === "video"
          ? video.tracks.video.list
          : video.tracks.audio.list;

      // here we gotta basically do the opposite of what we do in Settings
      const newTrack = player.getBitrateInfoListFor(e.mediaType)[e.newQuality];
      const realTrack = tracks.filter(
        (track) =>
          track.bandwidth === newTrack.bitrate &&
          parseInt(track.height) === newTrack.height
      )[0];

      dispatch(
        updateTrack(e.mediaType, {
          current: tracks.indexOf(realTrack),
        })
      );
    },
    [dispatch, pendingVideoSwitch, player, video]
  );

  const eTrackChange = useCallback(
    (e) => {
      console.log("[video] track changing ", e);

      if (e.mediaType !== "audio") return;

      const tracks = video.tracks.audio.list;
      const realTrack = tracks.filter(
        (track) => track.set_id === e.newMediaInfo.id
      )[0];

      dispatch(
        updateTrack(e.mediaType, {
          current: tracks.indexOf(realTrack),
        })
      );
    },
    [dispatch, video]
  );

  // other events
  useEffect(() => {
    if (!player) return;

    player.on(MediaPlayer.events.MANIFEST_LOADED, eManifestLoad);
    player.on(MediaPlayer.events.CAN_PLAY, eCanPlay);
    player.on(MediaPlayer.events.ERROR, eError);

    return () => {
      player.off(MediaPlayer.events.MANIFEST_LOADED, eManifestLoad);
      player.off(MediaPlayer.events.CAN_PLAY, eCanPlay);
      player.off(MediaPlayer.events.ERROR, eError);
    };
  }, [eCanPlay, eError, eManifestLoad, player]);

  // video playback
  useEffect(() => {
    if (!player) return;

    player.on(MediaPlayer.events.PLAYBACK_PAUSED, ePlayBackPaused);
    player.on(MediaPlayer.events.PLAYBACK_PLAYING, ePlayBackPlaying);
    player.on(MediaPlayer.events.PLAYBACK_WAITING, ePlayBackWaiting);
    player.on(MediaPlayer.events.PLAYBACK_TIME_UPDATED, ePlayBackTimeUpdated);
    player.on(MediaPlayer.events.PLAYBACK_NOT_ALLOWED, ePlayBackNotAllowed);
    player.on(MediaPlayer.events.PLAYBACK_ENDED, ePlayBackEnded);
    player.on(MediaPlayer.events.PLAYBACK_SEEKED, ePlaybackSeeked);
    player.on(MediaPlayer.events.QUALITY_CHANGE_RENDERED, eQualityChange);
    player.on(MediaPlayer.events.TRACK_CHANGE_RENDERED, eTrackChange);

    return () => {
      player.off(MediaPlayer.events.PLAYBACK_PAUSED, ePlayBackPaused);
      player.off(MediaPlayer.events.PLAYBACK_PLAYING, ePlayBackPlaying);
      player.off(MediaPlayer.events.PLAYBACK_WAITING, ePlayBackWaiting);
      player.off(
        MediaPlayer.events.PLAYBACK_TIME_UPDATED,
        ePlayBackTimeUpdated
      );
      player.off(MediaPlayer.events.PLAYBACK_NOT_ALLOWED, ePlayBackNotAllowed);
      player.off(MediaPlayer.events.PLAYBACK_ENDED, ePlayBackEnded);
      player.off(MediaPlayer.events.PLAYBACK_SEEKED, ePlaybackSeeked);
      player.off(MediaPlayer.events.QUALITY_CHANGE_RENDERED, eQualityChange);
      player.off(MediaPlayer.events.TRACK_CHANGE_RENDERED, eTrackChange);
    };
  }, [
    ePlayBackEnded,
    ePlayBackNotAllowed,
    ePlayBackPaused,
    ePlayBackPlaying,
    ePlaybackSeeked,
    ePlayBackTimeUpdated,
    ePlayBackWaiting,
    eQualityChange,
    eTrackChange,
    player,
  ]);

  return null;
}

export default VideoEvents;
