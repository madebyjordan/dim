import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { useParams } from "react-router";

import AirPlayIcon from "../../../../assets/Icons/AirPlay";
import { updateVideo } from "../../../../actions/video";
import {
  useCreatePlaybackSessionMutation,
  useKillPlaybackSessionMutation,
} from "../../../../api/v1/foundation";
import { VideoPlayerContext } from "../../Context";
import { useAppDispatch } from "../../../../hooks/store";

interface WebKitAirPlayVideo extends HTMLVideoElement {
  webkitShowPlaybackTargetPicker?: () => void;
  webkitCurrentPlaybackTargetIsWireless?: boolean;
}

interface AvailabilityEvent extends Event {
  availability?: "available" | "not-available";
}

const EMPTY_CAPABILITIES = { video: null, audio: [] };

export default function AirPlay() {
  const dispatch = useAppDispatch();
  const { fileID } = useParams();
  const { player, setRemoteMedia, videoRef } = useContext(VideoPlayerContext)!;
  const [createPlaybackSession] = useCreatePlaybackSessionMutation();
  const [killPlaybackSession] = useKillPlaybackSessionMutation();
  const remoteRef = useRef<WebKitAirPlayVideo | null>(null);
  const [available, setAvailable] = useState(false);
  const [session, setSession] = useState<{
    gid: string;
    url: string;
  } | null>(null);

  const receiverReachable =
    typeof window !== "undefined" &&
    !["localhost", "127.0.0.1", "::1"].includes(window.location.hostname);
  const webkitSupported =
    typeof HTMLMediaElement !== "undefined" &&
    "webkitShowPlaybackTargetPicker" in HTMLMediaElement.prototype;
  const supported = receiverReachable && webkitSupported;

  useEffect(() => {
    if (!supported || !fileID) return;
    let cancelled = false;
    let createdGid: string | null = null;
    void (async () => {
      try {
        const payload = await createPlaybackSession({
          fileId: fileID,
          forceAss: false,
          capabilities: EMPTY_CAPABILITIES,
          target: "airplay",
        }).unwrap();
        const resource = payload.remote;
        if (cancelled || !resource?.url) {
          if (payload.gid) void killPlaybackSession(payload.gid);
          return;
        }
        createdGid = payload.gid;
        setSession({
          gid: payload.gid,
          url: new URL(resource.url, window.location.origin).href,
        });
      } catch (error) {
        console.error("[AIRPLAY] failed to prepare playback", error);
      }
    })();
    return () => {
      cancelled = true;
      if (createdGid) void killPlaybackSession(createdGid);
    };
  }, [createPlaybackSession, fileID, killPlaybackSession, supported]);

  useEffect(() => {
    const remote = remoteRef.current;
    if (!remote || !session) return;
    remote.setAttribute("x-webkit-airplay", "allow");
    remote.src = session.url;

    const availabilityChanged = (event: Event) => {
      setAvailable(
        (event as AvailabilityEvent).availability === "available"
      );
    };
    const routeChanged = () => {
      const wireless = remote.webkitCurrentPlaybackTargetIsWireless === true;
      if (wireless) {
        const local = videoRef.current;
        const shouldPlay = local ? !local.paused : true;
        const position = local?.currentTime || 0;
        local?.pause();
        if (Number.isFinite(position)) remote.currentTime = position;
        setRemoteMedia(remote);
        if (shouldPlay) void remote.play();
      } else {
        const position = remote.currentTime;
        const shouldPlay = !remote.paused && !remote.ended;
        setRemoteMedia(null);
        if (Number.isFinite(position)) player?.seek(position);
        if (shouldPlay) player?.play();
      }
    };
    const timeChanged = () => {
      if (!remote.webkitCurrentPlaybackTargetIsWireless) return;
      const buffered = remote.buffered.length
        ? Math.max(0, remote.buffered.end(remote.buffered.length - 1) - remote.currentTime)
        : 0;
      dispatch(
        updateVideo({
          currentTime: Math.floor(remote.currentTime),
          duration: Math.round(remote.duration) || 0,
          buffer: Math.round(buffered),
        })
      );
    };
    const playing = () =>
      dispatch(updateVideo({ paused: false, waiting: false, canPlay: true }));
    const paused = () => dispatch(updateVideo({ paused: true }));
    const waiting = () => dispatch(updateVideo({ waiting: true }));
    const ended = () => dispatch(updateVideo({ playback_ended: true }));
    const failed = () =>
      dispatch(
        updateVideo({
          waiting: false,
          error: { msg: "AirPlay could not play this media." },
        })
      );

    remote.addEventListener(
      "webkitplaybacktargetavailabilitychanged",
      availabilityChanged
    );
    remote.addEventListener(
      "webkitcurrentplaybacktargetiswirelesschanged",
      routeChanged
    );
    remote.addEventListener("timeupdate", timeChanged);
    remote.addEventListener("durationchange", timeChanged);
    remote.addEventListener("playing", playing);
    remote.addEventListener("pause", paused);
    remote.addEventListener("waiting", waiting);
    remote.addEventListener("ended", ended);
    remote.addEventListener("error", failed);
    remote.load();
    return () => {
      setRemoteMedia(null);
      remote.pause();
      remote.removeAttribute("src");
      remote.load();
      remote.removeEventListener(
        "webkitplaybacktargetavailabilitychanged",
        availabilityChanged
      );
      remote.removeEventListener(
        "webkitcurrentplaybacktargetiswirelesschanged",
        routeChanged
      );
      remote.removeEventListener("timeupdate", timeChanged);
      remote.removeEventListener("durationchange", timeChanged);
      remote.removeEventListener("playing", playing);
      remote.removeEventListener("pause", paused);
      remote.removeEventListener("waiting", waiting);
      remote.removeEventListener("ended", ended);
      remote.removeEventListener("error", failed);
    };
  }, [dispatch, player, session, setRemoteMedia, videoRef]);

  const chooseTarget = useCallback(() => {
    remoteRef.current?.webkitShowPlaybackTargetPicker?.();
  }, []);

  if (!webkitSupported) return null;
  return (
    <>
      <button
        className={`airplay trackActive-${
          remoteRef.current?.webkitCurrentPlaybackTargetIsWireless === true
        }`}
        onClick={chooseTarget}
        disabled={!receiverReachable || !available || !session}
        title={
          !receiverReachable
            ? "Open Grin using this Mac's LAN address to use AirPlay"
            : available
              ? "Choose AirPlay target"
              : "No AirPlay target available"
        }
        aria-label="Choose AirPlay target"
      >
        <AirPlayIcon />
      </button>
      <video className="airplayMedia" ref={remoteRef} preload="metadata" playsInline />
    </>
  );
}
