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
  audio: AudioCapabilityRequest[];
  server_remux_supported: boolean;
  probe_source: "ingestion" | "fallback";
}

export interface AudioCapabilityRequest {
  stream_index: number;
  content_type: string;
  codec: string;
  codec_descriptor: string;
  channels: number;
  bitrate: number;
  sample_rate: number;
}

export interface BrowserVideoCapability {
  content_type: string;
  can_play_type: boolean;
  media_source: boolean;
  supported: boolean;
  smooth: boolean;
  power_efficient: boolean | null;
  hdr_display: boolean | null;
  can_play_type_result: "probably" | "maybe" | "unsupported";
  media_capabilities_result:
    | "supported"
    | "unsupported"
    | "unavailable"
    | "error";
}

export interface BrowserAudioCapability {
  stream_index: number;
  content_type: string;
  can_play_type: boolean;
  media_source: boolean;
  supported: boolean;
  smooth: boolean;
  power_efficient: boolean | null;
  can_play_type_result: "probably" | "maybe" | "unsupported";
  media_capabilities_result:
    | "supported"
    | "unsupported"
    | "unavailable"
    | "error";
}

export interface BrowserCapabilities {
  video: BrowserVideoCapability | null;
  audio: BrowserAudioCapability[];
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
    can_play_type_result: "unsupported",
    media_capabilities_result: "unavailable",
  };

  const video = document.createElement("video");
  const canPlayType = video.canPlayType(source.content_type);
  result.can_play_type = canPlayType === "probably";
  result.can_play_type_result = canPlayType || "unsupported";
  result.media_source =
    typeof MediaSource !== "undefined" &&
    MediaSource.isTypeSupported(source.content_type);

  if (!result.media_source || !navigator.mediaCapabilities) {
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
    result.media_capabilities_result = capability.supported
      ? "supported"
      : "unsupported";
  } catch {
    result.media_capabilities_result = "error";
  }

  return result;
};

export const determineAudioPlaybackCapability = async (
  source: AudioCapabilityRequest
): Promise<BrowserAudioCapability> => {
  const result: BrowserAudioCapability = {
    stream_index: source.stream_index,
    content_type: source.content_type,
    can_play_type: false,
    media_source: false,
    supported: false,
    smooth: false,
    power_efficient: null,
    can_play_type_result: "unsupported",
    media_capabilities_result: "unavailable",
  };
  const audio = document.createElement("audio");
  const canPlayType = audio.canPlayType(source.content_type);
  result.can_play_type = canPlayType === "probably";
  result.can_play_type_result = canPlayType || "unsupported";
  result.media_source =
    typeof MediaSource !== "undefined" &&
    MediaSource.isTypeSupported(source.content_type);
  if (!result.media_source || !navigator.mediaCapabilities) {
    return result;
  }
  try {
    const capability = await navigator.mediaCapabilities.decodingInfo({
      type: "media-source",
      audio: {
        contentType: source.content_type,
        channels: String(source.channels),
        bitrate: source.bitrate,
        samplerate: source.sample_rate,
      },
    });
    result.supported = capability.supported;
    result.smooth = capability.smooth;
    result.power_efficient = capability.powerEfficient;
    result.media_capabilities_result = capability.supported
      ? "supported"
      : "unsupported";
  } catch {
    result.media_capabilities_result = "error";
  }
  return result;
};

export const determinePlaybackCapabilities = async (
  inspection: PlaybackCapabilityInspection
): Promise<BrowserCapabilities> => ({
  video: await determineVideoPlaybackCapability(inspection.video),
  audio: await Promise.all(
    (inspection.audio ?? []).map(determineAudioPlaybackCapability)
  ),
});
