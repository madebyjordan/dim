export interface VideoCapabilityRequest {
  content_type: string;
  codec: string;
  codec_descriptor: string;
  width: number;
  height: number;
  bitrate: number;
  frame_rate: number;
  hdr: boolean;
  hdr_metadata_type?: "smpteSt2086" | "smpteSt2094-10" | "smpteSt2094-40";
  color_gamut?: "srgb" | "p3" | "rec2020";
  transfer_function?: "srgb" | "pq" | "hlg";
}

export interface PlaybackCapabilityInspection {
  video: VideoCapabilityRequest | null;
  server_remux_supported: boolean;
  probe_source: "ingestion" | "fallback";
}

export interface BrowserVideoCapability {
  content_type: string;
  can_play_type: boolean;
  media_source: boolean;
  supported: boolean;
  smooth: boolean;
  power_efficient: boolean | null;
  hdr_display: boolean | null;
}

export const determineVideoPlaybackCapability = async (
  source: VideoCapabilityRequest | null | undefined
): Promise<BrowserVideoCapability | null> => {
  if (!source) return null;

  const result: BrowserVideoCapability = {
    content_type: source.content_type,
    can_play_type: false,
    media_source: false,
    supported: false,
    smooth: false,
    power_efficient: null,
    hdr_display: source.hdr
      ? typeof matchMedia === "function" &&
        matchMedia("(dynamic-range: high)").matches
      : true,
  };

  const video = document.createElement("video");
  result.can_play_type = video.canPlayType(source.content_type) === "probably";
  result.media_source =
    typeof MediaSource !== "undefined" &&
    MediaSource.isTypeSupported(source.content_type);

  if (
    !result.can_play_type ||
    !result.media_source ||
    !navigator.mediaCapabilities
  ) {
    return result;
  }

  const configuration: MediaDecodingConfiguration = {
    type: "media-source",
    video: {
      contentType: source.content_type,
      width: source.width,
      height: source.height,
      bitrate: source.bitrate,
      framerate: source.frame_rate,
      ...(source.hdr_metadata_type && {
        hdrMetadataType: source.hdr_metadata_type,
      }),
      ...(source.color_gamut && { colorGamut: source.color_gamut }),
      ...(source.transfer_function && {
        transferFunction: source.transfer_function,
      }),
    },
  };

  try {
    const capability = await navigator.mediaCapabilities.decodingInfo(
      configuration
    );
    result.supported = capability.supported;
    result.smooth = capability.smooth;
    result.power_efficient = capability.powerEfficient;
  } catch {
    // The absence of complete capability evidence is intentionally a conservative fallback.
  }

  return result;
};
