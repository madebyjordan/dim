import type {
  AudioCapabilityRequest,
  PlaybackCapabilityInspection
} from '$lib/api/generated';

export interface BrowserCapabilities {
  video: Record<string, unknown> | null;
  audio: Array<Record<string, unknown>>;
}

async function audioCapability(source: AudioCapabilityRequest) {
  const audio = document.createElement('audio');
  const canPlayType = audio.canPlayType(source.content_type);
  const mediaSource =
    typeof MediaSource !== 'undefined' &&
    MediaSource.isTypeSupported(source.content_type);
  let result: MediaCapabilitiesDecodingInfo | null = null;
  try {
    result =
      mediaSource && navigator.mediaCapabilities
        ? await navigator.mediaCapabilities.decodingInfo({
            type: 'media-source',
            audio: {
              contentType: source.content_type,
              channels: String(source.channels),
              bitrate: source.bitrate,
              samplerate: source.sample_rate
            }
          })
        : null;
  } catch {
    /* capability errors are evidence, not playback failures */
  }
  return {
    stream_index: source.stream_index,
    content_type: source.content_type,
    can_play_type: canPlayType === 'probably',
    media_source: mediaSource,
    supported: result?.supported ?? false,
    smooth: result?.smooth ?? false,
    power_efficient: result?.powerEfficient ?? null,
    can_play_type_result: canPlayType || 'unsupported',
    media_capabilities_result: result
      ? result.supported
        ? 'supported'
        : 'unsupported'
      : 'unavailable'
  };
}

export async function determineCapabilities(
  source: PlaybackCapabilityInspection
): Promise<BrowserCapabilities> {
  const video = source.video;
  let videoResult: Record<string, unknown> | null = null;
  if (video?.content_type) {
    const element = document.createElement('video');
    const canPlayType = element.canPlayType(video.content_type);
    const mediaSource =
      typeof MediaSource !== 'undefined' &&
      MediaSource.isTypeSupported(video.content_type);
    let result: MediaCapabilitiesDecodingInfo | null = null;
    try {
      result =
        mediaSource && navigator.mediaCapabilities
          ? await navigator.mediaCapabilities.decodingInfo({
              type: 'media-source',
              video: {
                contentType: video.content_type,
                width: video.width ?? 1,
                height: video.height ?? 1,
                bitrate: video.bitrate ?? 1,
                framerate: video.frame_rate ?? 30
              }
            })
          : null;
    } catch {
      /* capability errors are reported as unavailable */
    }
    videoResult = {
      content_type: video.content_type,
      can_play_type: canPlayType === 'probably',
      media_source: mediaSource,
      supported: result?.supported ?? false,
      smooth: result?.smooth ?? false,
      power_efficient: result?.powerEfficient ?? null,
      hdr_display: video.hdr
        ? matchMedia('(dynamic-range: high)').matches
        : true,
      can_play_type_result: canPlayType || 'unsupported',
      media_capabilities_result: result
        ? result.supported
          ? 'supported'
          : 'unsupported'
        : 'unavailable'
    };
  }
  return {
    video: videoResult,
    audio: await Promise.all(source.audio.map(audioCapability))
  };
}
