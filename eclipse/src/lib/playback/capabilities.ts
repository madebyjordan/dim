import type {
  AudioCapabilityRequest,
  PlaybackCapabilityInspection
} from '$lib/api/generated';

export interface BrowserCapabilities {
  video: Record<string, unknown> | null;
  audio: Array<Record<string, unknown>>;
}

export type CapabilityProbeEvent = {
  phase: 'start' | 'end';
  kind: 'video' | 'audio';
  api: 'browser-report' | 'media-capabilities';
  contentType: string;
  streamIndex?: number;
  result?: Record<string, unknown>;
  failure?: string;
};

type CapabilityProbeOptions = {
  onEvent?: (event: CapabilityProbeEvent) => void;
};

async function audioCapability(
  source: AudioCapabilityRequest,
  onEvent?: CapabilityProbeOptions['onEvent']
) {
  const audio = document.createElement('audio');
  const canPlayType = audio.canPlayType(source.content_type);
  const mediaSource =
    typeof MediaSource !== 'undefined' &&
    MediaSource.isTypeSupported(source.content_type);
  onEvent?.({
    phase: 'end',
    kind: 'audio',
    api: 'browser-report',
    contentType: source.content_type,
    streamIndex: source.stream_index,
    result: { canPlayType: canPlayType || 'unsupported', mediaSource }
  });
  let result: MediaCapabilitiesDecodingInfo | null = null;
  try {
    if (mediaSource && navigator.mediaCapabilities) {
      onEvent?.({
        phase: 'start',
        kind: 'audio',
        api: 'media-capabilities',
        contentType: source.content_type,
        streamIndex: source.stream_index
      });
      result = await navigator.mediaCapabilities.decodingInfo({
        type: 'media-source',
        audio: {
          contentType: source.content_type,
          channels: String(source.channels),
          bitrate: source.bitrate,
          samplerate: source.sample_rate
        }
      });
      onEvent?.({
        phase: 'end',
        kind: 'audio',
        api: 'media-capabilities',
        contentType: source.content_type,
        streamIndex: source.stream_index,
        result: {
          supported: result.supported,
          smooth: result.smooth,
          powerEfficient: result.powerEfficient
        }
      });
    }
  } catch (cause) {
    onEvent?.({
      phase: 'end',
      kind: 'audio',
      api: 'media-capabilities',
      contentType: source.content_type,
      streamIndex: source.stream_index,
      failure: cause instanceof Error ? cause.message : String(cause)
    });
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
  source: PlaybackCapabilityInspection,
  options: CapabilityProbeOptions = {}
): Promise<BrowserCapabilities> {
  const video = source.video;
  let videoResult: Record<string, unknown> | null = null;
  if (video?.content_type) {
    const element = document.createElement('video');
    const canPlayType = element.canPlayType(video.content_type);
    const mediaSource =
      typeof MediaSource !== 'undefined' &&
      MediaSource.isTypeSupported(video.content_type);
    options.onEvent?.({
      phase: 'end',
      kind: 'video',
      api: 'browser-report',
      contentType: video.content_type,
      result: { canPlayType: canPlayType || 'unsupported', mediaSource }
    });
    let result: MediaCapabilitiesDecodingInfo | null = null;
    try {
      if (mediaSource && navigator.mediaCapabilities) {
        options.onEvent?.({
          phase: 'start',
          kind: 'video',
          api: 'media-capabilities',
          contentType: video.content_type
        });
        result = await navigator.mediaCapabilities.decodingInfo({
          type: 'media-source',
          video: {
            contentType: video.content_type,
            width: video.width ?? 1,
            height: video.height ?? 1,
            bitrate: video.bitrate ?? 1,
            framerate: video.frame_rate ?? 30
          }
        });
        options.onEvent?.({
          phase: 'end',
          kind: 'video',
          api: 'media-capabilities',
          contentType: video.content_type,
          result: {
            supported: result.supported,
            smooth: result.smooth,
            powerEfficient: result.powerEfficient
          }
        });
      }
    } catch (cause) {
      options.onEvent?.({
        phase: 'end',
        kind: 'video',
        api: 'media-capabilities',
        contentType: video.content_type,
        failure: cause instanceof Error ? cause.message : String(cause)
      });
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
    audio: await Promise.all(
      source.audio.map((audio) => audioCapability(audio, options.onEvent))
    )
  };
}
