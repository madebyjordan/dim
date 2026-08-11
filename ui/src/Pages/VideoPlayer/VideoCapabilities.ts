export const VERIFIED_AV1_CONTENT_TYPE =
  'video/mp4; codecs="av01.0.08M.10.0.111.01.01.01.0"';

export const supportsVerifiedAv1Playback = async () => {
  // Runtime evidence currently covers Chrome/Chromium 151 only. Other engines and versions
  // retain the conservative server fallback until they are independently verified.
  const verifiedChromium =
    /Macintosh; Intel Mac OS X 10_15_7/.test(navigator.userAgent) &&
    / Chrome\/151\./.test(navigator.userAgent) &&
    !/ (?:Edg|OPR)\//.test(navigator.userAgent);
  if (
    !verifiedChromium ||
    typeof MediaSource === "undefined" ||
    !navigator.mediaCapabilities
  ) {
    return false;
  }

  const video = document.createElement("video");
  if (
    video.canPlayType(VERIFIED_AV1_CONTENT_TYPE) !== "probably" ||
    !MediaSource.isTypeSupported(VERIFIED_AV1_CONTENT_TYPE)
  ) {
    return false;
  }

  try {
    const result = await navigator.mediaCapabilities.decodingInfo({
      type: "media-source",
      video: {
        contentType: VERIFIED_AV1_CONTENT_TYPE,
        width: 1920,
        height: 1080,
        bitrate: 6_300_000,
        framerate: 24,
      },
    });
    return result.supported && result.smooth;
  } catch {
    return false;
  }
};
