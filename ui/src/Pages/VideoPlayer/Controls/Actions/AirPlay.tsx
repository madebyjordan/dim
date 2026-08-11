import {
  type KeyboardEvent,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useParams } from "react-router";

import AirPlayIcon from "../../../../assets/Icons/AirPlay";
import { updateVideo } from "../../../../actions/video";
import {
  useCreatePlaybackSessionMutation,
} from "../../../../api/v1/foundation";
import { apiRequest } from "../../../../api/transport";
import { VideoPlayerContext } from "../../Context";
import { useAppDispatch, useAppSelector } from "../../../../hooks/store";

interface WebKitAirPlayVideo extends HTMLVideoElement {
  webkitShowPlaybackTargetPicker?: () => void;
  webkitCurrentPlaybackTargetIsWireless?: boolean;
}

interface AvailabilityEvent extends Event {
  availability?: "available" | "not-available";
}

const mediaErrorName = (error: MediaError | null) => {
  switch (error?.code) {
    case MediaError.MEDIA_ERR_ABORTED:
      return "playback aborted";
    case MediaError.MEDIA_ERR_NETWORK:
      return "network request failed";
    case MediaError.MEDIA_ERR_DECODE:
      return "receiver could not decode the media";
    case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
      return "media source was rejected";
    default:
      return "unknown media error";
  }
};

const EMPTY_CAPABILITIES = { video: null, audio: [] };

export default function AirPlay() {
  const dispatch = useAppDispatch();
  const authToken = useAppSelector((state) => state.auth.token);
  const { fileID } = useParams();
  const { player, setRemoteMedia, videoRef } = useContext(VideoPlayerContext)!;
  const [createPlaybackSession] = useCreatePlaybackSessionMutation();
  const remoteRef = useRef<WebKitAirPlayVideo | null>(null);
  const sessionGidRef = useRef<string | null>(null);
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

  const terminateRemoteSession = useCallback(
    (gid: string, reason: string) => {
      console.info("[AIRPLAY] terminating playback session", {
        sessionId: gid,
        reason,
      });
      void apiRequest(`stream/${gid}/state/kill`, {
        method: "DELETE",
        token: authToken,
        keepalive: true,
      })
        .then(() =>
          console.info("[AIRPLAY] playback session terminated", {
            sessionId: gid,
            reason,
          })
        )
        .catch((error) =>
          console.error("[AIRPLAY] failed to terminate playback session", {
            sessionId: gid,
            reason,
            error,
          })
        );
    },
    [authToken]
  );

  useEffect(() => {
    const pageHidden = () => {
      const gid = sessionGidRef.current;
      if (!gid) return;
      sessionGidRef.current = null;
      terminateRemoteSession(gid, "page hidden");
    };
    window.addEventListener("pagehide", pageHidden);
    return () => window.removeEventListener("pagehide", pageHidden);
  }, [terminateRemoteSession]);

  useEffect(() => {
    if (!supported || !fileID) return;
    setSession(null);
    const previousGid = sessionGidRef.current;
    if (previousGid) {
      sessionGidRef.current = null;
      terminateRemoteSession(previousGid, "playback source changed");
    }
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
          if (payload.gid) terminateRemoteSession(payload.gid, "cancelled");
          return;
        }
        createdGid = payload.gid;
        sessionGidRef.current = payload.gid;
        console.info("[AIRPLAY] playback session prepared", {
          sessionId: payload.gid,
        });
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
      if (createdGid && sessionGidRef.current === createdGid) {
        sessionGidRef.current = null;
        terminateRemoteSession(createdGid, "component cleanup");
      }
    };
  }, [createPlaybackSession, fileID, supported, terminateRemoteSession]);

  useEffect(() => {
    const remote = remoteRef.current;
    if (!remote || !session) return;
    remote.setAttribute("x-webkit-airplay", "allow");
    remote.preload = "none";
    remote.src = session.url;

    const availabilityChanged = (event: Event) => {
      const availability = (event as AvailabilityEvent).availability;
      console.info("[AIRPLAY] target availability changed", { availability });
      setAvailable(availability === "available");
    };
    const routeChanged = () => {
      const wireless = remote.webkitCurrentPlaybackTargetIsWireless === true;
      console.info("[AIRPLAY] playback route changed", {
        sessionId: session.gid,
        wireless,
        networkState: remote.networkState,
        readyState: remote.readyState,
      });
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
    const failed = () => {
      const reason = mediaErrorName(remote.error);
      const stage = remote.webkitCurrentPlaybackTargetIsWireless
        ? "wireless receiver playback"
        : "media preparation before receiver selection";
      console.error("[AIRPLAY] media element failed", {
        sessionId: session.gid,
        stage,
        reason,
        mediaErrorCode: remote.error?.code ?? 0,
        mediaErrorMessage: remote.error?.message ?? "",
        networkState: remote.networkState,
        readyState: remote.readyState,
        currentSrc: remote.currentSrc,
      });
      dispatch(
        updateVideo({
          waiting: false,
          error: { msg: `AirPlay failed during ${stage}: ${reason}.` },
        })
      );
    };

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

  const prepareTargetPicker = useCallback(() => {
    const remote = remoteRef.current;
    if (!remote || remote.preload !== "none") return;

    // Safari resets the picker request when load() is called from the click
    // handler itself. Initialise on pointer-down so the subsequent click can
    // remain a pure, user-initiated picker request.
    remote.preload = "metadata";
    remote.load();
  }, []);

  const prepareTargetPickerFromKeyboard = useCallback(
    (event: KeyboardEvent<HTMLButtonElement>) => {
      if (event.key === "Enter" || event.key === " ") prepareTargetPicker();
    },
    [prepareTargetPicker]
  );

  const chooseTarget = useCallback(() => {
    const remote = remoteRef.current;
    console.info("[AIRPLAY] target picker requested", {
      sessionId: session?.gid,
      networkState: remote?.networkState,
      readyState: remote?.readyState,
    });
    if (!remote) return;
    const showPicker = () => {
      console.info("[AIRPLAY] media prepared for target picker", {
        sessionId: session?.gid,
        networkState: remote.networkState,
        readyState: remote.readyState,
      });
      remote.webkitShowPlaybackTargetPicker?.();
    };

    showPicker();
  }, [session?.gid]);

  if (!webkitSupported) return null;
  return (
    <>
      <button
        className={`airplay trackActive-${
          remoteRef.current?.webkitCurrentPlaybackTargetIsWireless === true
        }`}
        onPointerDown={prepareTargetPicker}
        onKeyDown={prepareTargetPickerFromKeyboard}
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
      <video className="airplayMedia" ref={remoteRef} preload="none" playsInline />
    </>
  );
}
