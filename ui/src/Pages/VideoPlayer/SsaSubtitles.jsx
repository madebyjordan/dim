import { useEffect, useState, useContext } from "react";
import { shallowEqual, useSelector } from "react-redux";

import { VideoPlayerContext } from "./Context";

import JASSUB from "jassub";

import "./Subtitles.scss";

function VideoSubtitles() {
  const { token, video, subtitle } = useSelector(
    (store) => ({
      token: store.auth.token,
      video: store.video,
      subtitle: store.video.tracks.subtitle,
    }),
    shallowEqual
  );

  const currentSub = subtitle.list[subtitle.current];

  const isAssEnabled = localStorage.getItem("enable_ssa") === "true";
  const isAss = !!(isAssEnabled && currentSub?.chunk_path?.endsWith("ass"));
  const [jassub, setJASSUB] = useState();
  const [subContent, setSubContent] = useState();
  const { videoRef } = useContext(VideoPlayerContext);

  useEffect(() => {
    if (!isAss || !currentSub) {
      setSubContent(null);
      return;
    }

    let cancelled = false;
    const chunkPath = `/api/v1/stream/${currentSub.chunk_path}`;
    setSubContent(null);

    (async () => {
      try {
        const response = await fetch(chunkPath, {
          headers: { Authorization: token },
        });

        if (response.ok && !cancelled) {
          setSubContent(await response.text());
        }
      } catch (error) {
        if (!cancelled) {
          console.error("[subtitle] failed to load ASS subtitle", error);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [currentSub, isAss, token]);

  useEffect(() => {
    if (
      jassub ||
      !video.textTrackEnabled ||
      video.prevSubs === subtitle.current ||
      !isAss ||
      !subContent ||
      !videoRef
    )
      return;

    console.log("[INFO] Loading ASS subtitle");

    JASSUB._test();

    const options = {
      video: videoRef.current,
      subContent,
      dropAllBlur: !JASSUB._supportsSIMD,
      workerUrl: new URL(
        "jassub/dist/jassub-worker.js",
        import.meta.url
      ).toString(),
      wasmUrl: new URL(
        "jassub/dist/jassub-worker.wasm",
        import.meta.url
      ).toString(),
      modernWasmUrl: new URL(
        "jassub/dist/jassub-worker-modern.wasm",
        import.meta.url
      ).toString(),
      availableFonts: { "liberation sans": "/static/default.woff2" },
      fonts: ["/static/default.woff2"],
    };

    setJASSUB(new JASSUB(options));

    return () => {
      console.log("[subtitle] disposing of jassub ctx");
      if (jassub) jassub.destroy();
    };
  }, [video, videoRef, subtitle, isAss, subContent, setJASSUB, jassub]);

  useEffect(() => {
    if (
      !jassub ||
      !video.textTrackEnabled ||
      video.prevSubs === subtitle.current ||
      !isAss ||
      !subContent
    )
      return;

    jassub.setTrack(subContent);
  }, [
    jassub,
    video.textTrackEnabled,
    video.prevSubs,
    subtitle,
    isAss,
    subContent,
  ]);

  useEffect(() => {
    if (jassub && !isAss) {
      console.log("[subtitle] disposing of jassub ctx");
      jassub.destroy();
      setJASSUB(null);
    }
  }, [jassub, setJASSUB, isAss]);

  return null;
}

export default VideoSubtitles;
