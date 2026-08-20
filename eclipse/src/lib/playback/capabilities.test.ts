// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PlaybackCapabilityInspection } from '$lib/api/generated';
import {
  determineCapabilities,
  type CapabilityProbeEvent
} from './capabilities';

const inspection: PlaybackCapabilityInspection = {
  video: {
    content_type: 'video/mp4; codecs="avc1.640028"',
    codec: 'h264',
    codec_descriptor: 'avc1.640028',
    width: 1920,
    height: 1080,
    bitrate: 8_000_000,
    frame_rate: 24,
    hdr: false
  },
  audio: [
    {
      stream_index: 1,
      content_type: 'audio/mp4; codecs="mp4a.40.2"',
      codec: 'aac',
      codec_descriptor: 'mp4a.40.2',
      channels: 6,
      bitrate: 384_000,
      sample_rate: 48_000
    }
  ],
  server_remux_supported: true,
  probe_source: 'ingestion'
};

function installMediaApis(
  decodingInfo: (
    configuration: MediaDecodingConfiguration
  ) => Promise<MediaCapabilitiesDecodingInfo>
) {
  vi.spyOn(HTMLMediaElement.prototype, 'canPlayType').mockReturnValue(
    'probably'
  );
  vi.stubGlobal('MediaSource', { isTypeSupported: vi.fn(() => true) });
  Object.defineProperty(navigator, 'mediaCapabilities', {
    configurable: true,
    value: { decodingInfo: vi.fn(decodingInfo) }
  });
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, 'mediaCapabilities');
});

describe('browser playback capabilities', () => {
  it('reports exact H.264 and multichannel AAC media-source evidence', async () => {
    const events: CapabilityProbeEvent[] = [];
    installMediaApis(async () => ({
      supported: true,
      smooth: true,
      powerEfficient: false,
      keySystemAccess: null,
      configuration: {} as MediaDecodingConfiguration
    }));

    const result = await determineCapabilities(inspection, {
      onEvent: (event) => events.push(event)
    });

    expect(result.video).toEqual(
      expect.objectContaining({
        can_play_type: true,
        media_source: true,
        supported: true,
        smooth: true
      })
    );
    expect(result.audio[0]).toEqual(
      expect.objectContaining({
        stream_index: 1,
        can_play_type: true,
        media_source: true,
        supported: true,
        smooth: true
      })
    );
    expect(events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          phase: 'start',
          kind: 'video',
          api: 'media-capabilities'
        }),
        expect.objectContaining({
          phase: 'end',
          kind: 'audio',
          streamIndex: 1,
          api: 'media-capabilities'
        })
      ])
    );
  });

  it('surfaces rejected MediaCapabilities evidence as unavailable', async () => {
    const events: CapabilityProbeEvent[] = [];
    installMediaApis(async () => {
      throw new Error('WebKit rejected the configuration');
    });

    const result = await determineCapabilities(inspection, {
      onEvent: (event) => events.push(event)
    });

    expect(result.video).toEqual(
      expect.objectContaining({ media_capabilities_result: 'unavailable' })
    );
    expect(result.audio[0]).toEqual(
      expect.objectContaining({ media_capabilities_result: 'unavailable' })
    );
    expect(events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          phase: 'end',
          api: 'media-capabilities',
          failure: 'WebKit rejected the configuration'
        })
      ])
    );
  });
});
