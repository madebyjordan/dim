import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router";
import { shallowEqual, useDispatch, useSelector } from "react-redux";
import { skipToken } from "@reduxjs/toolkit/query/react";
import { MediaPlayer, Debug } from "dashjs";
import {
  setTracks,
  setGID,
  setManifestState,
  updateVideo,
  incIdleCount,
  clearVideoData,
} from "../../actions/video";
import { fetchUserSettings } from "../../actions/settings.js";
import { useGetMediaFilesQuery, useGetMediaQuery } from "../../api/v1/media";
import { VideoPlayerContext } from "./Context";
import VideoEvents from "./Events";
import VideoMediaData from "./MediaData";

import RingLoad from "../../Components/Load/Ring";
import Menus from "./Menus/Index";
import VideoControls from "./Controls/Index";
import ErrorBox from "./ErrorBox";
import ContinueProgress from "./ContinueProgress";
import VttSubtitles from "./VttSubtitles";
import SsaSubtitles from "./SsaSubtitles";
import NextVideo from "./NextVideo/Index";
import BackButton from "./BackButton";
import { PLAYBACK_ERROR_MESSAGE } from "./PlaybackFailure";
import { buildPlaybackManifestUrl } from "./QualitySwitch";
import { createPlaybackState } from "./Navigation";
import { determinePlaybackCapabilities } from "./VideoCapabilities";
import {
  reclaimPlaybackSession,
  setPlaybackSession,
  terminatePlaybackSession,
} from "../../storage";
import {
  useCreatePlaybackSessionMutation,
  useKillPlaybackSessionMutation,
  useLazyInspectPlaybackCapabilitiesQuery,
} from "../../api/v1/foundation";

import "./Index.scss";

function VideoPlayer() {
  const params = useParams();
  const dispatch = useDispatch();
  const [createPlaybackSession] = useCreatePlaybackSessionMutation();
  const [killPlaybackSession] = useKillPlaybackSessionMutation();
  const [inspectPlaybackCapabilities] =
    useLazyInspectPlaybackCapabilitiesQuery();
  const location = useLocation();
  const navigate = useNavigate();
  const [player, setPlayer] = useState();

  const { error, manifest, audioTracks, videoTracks, video, auth, settings } =
    useSelector(
      (store) => ({
        auth: store.auth,
        video: store.video,
        manifest: store.video.manifest,
        videoTracks: store.video.tracks.video,
        audioTracks: store.video.tracks.audio,
        error: store.video.error,
        settings: store.settings,
      }),
      shallowEqual
    );

  const videoPlayer = useRef(null);
  const overlay = useRef(null);
  const videoRef = useRef(null);
  const pendingVideoSwitch = useRef(null);

  const { data: media } = useGetMediaQuery(
    video.mediaID ? video.mediaID : skipToken
  );
  const nextEpisodeId = media && media.next_episode_id;
  const { data: nextMediaFiles } = useGetMediaFilesQuery(
    nextEpisodeId ? nextEpisodeId : skipToken
  );

  useEffect(() => {
    if (media) {
      document.title = `Dim - Playing '${media.name}'`;
    } else {
      document.title = "Dim - Video Player";
    }
  }, [media]);

  // FIXME: Not sure where the best place to do this is, but we need userSettings, but sometimes the user navigates to /play directly so we never fetch userSettings
  useEffect(() => {
    if (settings.userSettings.fetching || settings.userSettings.fetched) return;

    dispatch(fetchUserSettings());
  }, [dispatch, settings.userSettings]);

  // If playback finished, redirect to the next video
  useEffect(() => {
    if (!settings?.userSettings?.data?.enable_autoplay) return;

    const item = nextMediaFiles && nextMediaFiles[0];

    if (!item) return;

    const ts_diff = video.currentTime - media.duration;
    if (video.playback_ended && ts_diff < 10) {
      navigate(`/play/${item.id}`, {
        replace: true,
        state: createPlaybackState(location),
      });
    }
  }, [
    media,
    nextMediaFiles,
    video.mediaID,
    video.currentTime,
    video.playback_ended,
    location,
    navigate,
    settings,
    settings.userSettings,
  ]);

  // Reset GID if play id changes so that this component loads a new video.
  useEffect(() => {
    dispatch(setGID(null));
  }, [params.fileID, dispatch]);

  useEffect(() => {
    if (video.gid) return;

    const force_ass = localStorage.getItem("enable_ssa") === "true";
    let cancelled = false;
    (async () => {
      try {
        const removeSession = (gid) => killPlaybackSession(gid).unwrap();
        await reclaimPlaybackSession(removeSession);
        if (cancelled) return;

        const inspection = await inspectPlaybackCapabilities(
          params.fileID
        ).unwrap();
        if (cancelled) return;
        const capabilities = await determinePlaybackCapabilities(inspection);
        const payload = await createPlaybackSession({
          fileId: params.fileID,
          forceAss: force_ass,
          capabilities,
        }).unwrap();
        if (!payload.gid || !Array.isArray(payload.tracks)) {
          throw new Error("Manifest response was incomplete");
        }
        if (cancelled) {
          await terminatePlaybackSession(payload.gid, removeSession).catch(
            () => undefined
          );
          return;
        }

        setPlaybackSession(payload.gid);
        dispatch(setGID(payload.gid));

        const tVideos = payload.tracks.filter(
          (track) => track.content_type === "video"
        );
        const tAudios = payload.tracks.filter(
          (track) => track.content_type === "audio"
        );
        const tSubtitles = payload.tracks.filter(
          (track) => track.content_type === "subtitle"
        );

        dispatch(
          setTracks({
            video: tVideos,
            audio: tAudios,
            subtitle: tSubtitles,
          })
        );

        dispatch(
          setManifestState({
            virtual: { loaded: true },
          })
        );
      } catch (error) {
        if (cancelled) return;
        console.error("[VIDEO] failed to create playback manifest", error);
        dispatch(
          setManifestState({
            loading: false,
            loaded: false,
          })
        );
        dispatch(
          updateVideo({
            canPlay: false,
            error: { msg: PLAYBACK_ERROR_MESSAGE },
            waiting: false,
          })
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    createPlaybackSession,
    dispatch,
    killPlaybackSession,
    inspectPlaybackCapabilities,
    params.fileID,
    video.gid,
  ]);

  useEffect(() => {
    if (!video.gid || !manifest.virtual.loaded) return;

    console.log("[video] loading manifest");

    dispatch(
      setManifestState({
        loading: true,
        loaded: false,
      })
    );

    // Manifest selection is also the backend's process admission boundary. Start only the
    // preferred video/audio tracks; alternate renditions remain inspectable but lazy.
    const preferredTrack = (tracks) =>
      tracks.find((track) => track.is_default) || tracks[0];
    const includes = [
      preferredTrack(videoTracks.list)?.id,
      preferredTrack(audioTracks.list)?.id,
    ]
      .filter(Boolean)
      .join(",");
    const [videoId, audioId] = includes.split(",");
    const url = buildPlaybackManifestUrl({
      audioId,
      gid: video.gid,
      videoId,
    });
    const mediaPlayer = MediaPlayer().create();

    let settings = {
      debug: {
        logLevel: Debug.LOG_LEVEL_DEBUG,
      },
      streaming: {
        /* FIXME: Disabling temporarily because the code for this function is unsound
        gaps: {
          enableSeekFix: true
        },
        */
        abr: {
          autoSwitchBitrate: {
            video: false,
          },
        },
      },
    };

    mediaPlayer.updateSettings(settings);
    mediaPlayer.extend("RequestModifier", function () {
      return {
        modifyRequestHeader: function (xhr) {
          xhr.setRequestHeader("Authorization", auth.token);
          return xhr;
        },
        modifyRequestURL: function (url) {
          return url;
        },
      };
    });

    const getInitialTrack = (trackArr) => {
      const trackList =
        trackArr[0].type === "video" ? videoTracks.list : audioTracks.list;
      const defaultTracks = trackList.filter((track) => track.is_default);
      const defaultTrack =
        defaultTracks && defaultTracks.length > 0
          ? defaultTracks[0]
          : trackList[0];
      const initialTracks = trackArr.filter(
        (x) => x.id === defaultTrack.set_id
      );
      console.log(
        `[${trackArr[0].type}] setting initial track to`,
        initialTracks
      );
      return initialTracks;
    };

    mediaPlayer.initialize(videoRef.current, url, true);
    mediaPlayer.setCustomInitialTrackSelectionFunction(getInitialTrack);

    setPlayer(mediaPlayer);

    return () => {
      dispatch(clearVideoData());
      mediaPlayer.destroy();

      if (!video.gid) return;

      void terminatePlaybackSession(video.gid, (gid) =>
        killPlaybackSession(gid).unwrap()
      ).catch(() => undefined);
    };
  }, [
    audioTracks.list,
    auth.token,
    dispatch,
    killPlaybackSession,
    manifest.virtual.loaded,
    video.gid,
    videoTracks.list,
    setPlayer,
  ]);

  const play = useCallback(() => {
    dispatch(
      updateVideo({
        idleCount: 0,
      })
    );

    videoRef.current.play();
  }, [dispatch, videoRef]);

  const pause = useCallback(() => {
    dispatch(
      updateVideo({
        idleCount: 0,
      })
    );
    videoRef.current.pause();
  }, [dispatch, videoRef]);

  const togglePlayer = useCallback(
    (e) => {
      if (!videoRef.current) return;
      if (
        e.target.closest(
          ".videoBack, .videoMenus, .videoControls, .modalBoxContainer, .ReactModalPortal"
        )
      )
        return;

      videoRef.current.paused ? play() : pause();
    },
    [play, pause, videoRef]
  );

  const seekTo = useCallback(
    (newTime) => {
      player.seek(newTime);

      dispatch(
        updateVideo({
          seeking: false,
          currentTime: newTime,
        })
      );
    },
    [dispatch, player]
  );

  const changeVideoQuality = useCallback(
    async (trackIndex) => {
      if (!player || pendingVideoSwitch.current) return;
      if (trackIndex === videoTracks.current) return;
      const target = videoTracks.list[trackIndex];
      if (!target) return;
      const audio =
        audioTracks.list[audioTracks.current] ||
        audioTracks.list.find((track) => track.is_default) ||
        audioTracks.list[0];
      const url = buildPlaybackManifestUrl({
        audioId: audio?.id,
        gid: video.gid,
        replaceVideo: true,
        videoId: target.id,
      });
      const pending = {
        position: videoRef.current?.currentTime || 0,
        targetIndex: trackIndex,
        targetSetId: target.set_id,
        wasPaused: videoRef.current?.paused ?? false,
      };
      pendingVideoSwitch.current = pending;

      try {
        // Preflight activation keeps the current rendition playing if admission or profile
        // creation fails. The same URL is then loaded by dash.js to make the prepared rendition
        // effective.
        const response = await fetch(url, {
          headers: { Authorization: auth.token },
        });
        if (!response.ok) {
          throw new Error(`quality activation failed (${response.status})`);
        }
        dispatch(updateVideo({ waiting: true }));
        player.attachSource(url);
      } catch (error) {
        pendingVideoSwitch.current = null;
        dispatch(updateVideo({ waiting: false }));
        console.error(
          "[video] quality switch failed; continuing current rendition",
          error
        );
      }
    },
    [audioTracks, auth.token, dispatch, player, video.gid, videoTracks]
  );

  useEffect(() => {
    if (video.showSubSwitcher) return;
    dispatch(incIdleCount());
  }, [video.currentTime, dispatch, video.showSubSwitcher]);

  const initialValue = {
    videoRef,
    videoPlayer,
    overlay: overlay.current,
    seekTo,
    player,
    pendingVideoSwitch,
    changeVideoQuality,
  };

  const showNextVideoAfter = (media && media.chapters?.credits) || 0;

  return (
    <VideoPlayerContext.Provider value={initialValue}>
      <div className="videoPlayer" ref={videoPlayer} onClick={togglePlayer}>
        <VideoEvents />
        <VideoMediaData />
        <video ref={videoRef} />
        <VttSubtitles />
        <SsaSubtitles />
        <div className="overlay" ref={overlay}>
          <BackButton />
          {!error && manifest.loaded && video.canPlay && <Menus />}
          {!error && manifest.loaded && video.canPlay && nextEpisodeId && (
            <NextVideo id={nextEpisodeId} showAfter={showNextVideoAfter} />
          )}
          {!error && manifest.loaded && video.canPlay && <VideoControls />}
          {!error && (manifest.loading || !video.canPlay || video.waiting) && (
            <RingLoad />
          )}
          {!error &&
            manifest.loaded &&
            video.canPlay &&
            media &&
            media.progress > 0 && <ContinueProgress />}
          {error && <ErrorBox />}
        </div>
      </div>
    </VideoPlayerContext.Provider>
  );
}

export default VideoPlayer;
