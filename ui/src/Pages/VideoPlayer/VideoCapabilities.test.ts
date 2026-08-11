import {
  determineVideoPlaybackCapability,
  type VideoCapabilityRequest,
} from "./VideoCapabilities";

const friday: VideoCapabilityRequest = {
  content_type: 'video/mp4; codecs="av01.0.08M.10.0.111.01.01.01.0"',
  codec: "av1",
  codec_descriptor: "av01.0.08M.10.0.111.01.01.01.0",
  width: 1920,
  height: 1080,
  bitrate: 6_277_855,
  frame_rate: 24000 / 1001,
  hdr: false,
  color_gamut: "srgb",
  transfer_function: "srgb",
};

const matrix: VideoCapabilityRequest = {
  ...friday,
  content_type: 'video/mp4; codecs="av01.0.12M.10.0.110.09.16.09.0"',
  codec_descriptor: "av01.0.12M.10.0.110.09.16.09.0",
  width: 3840,
  height: 1600,
  bitrate: 11_618_576,
  hdr: true,
  hdr_metadata_type: "smpteSt2086",
  color_gamut: "rec2020",
  transfer_function: "pq",
};

beforeEach(() => {
  vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue(
    "probably"
  );
  vi.stubGlobal("MediaSource", { isTypeSupported: vi.fn(() => true) });
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: true }))
  );
  Object.defineProperty(navigator, "mediaCapabilities", {
    configurable: true,
    value: {
      decodingInfo: vi.fn().mockResolvedValue({
        supported: true,
        smooth: true,
        powerEfficient: true,
      }),
    },
  });
});

afterEach(() => vi.restoreAllMocks());

it.each([friday, matrix])(
  "queries the exact source configuration for $codec_descriptor",
  async (source) => {
    await expect(determineVideoPlaybackCapability(source)).resolves.toEqual({
      content_type: source.content_type,
      can_play_type: true,
      media_source: true,
      supported: true,
      smooth: true,
      power_efficient: true,
      hdr_display: true,
    });
    expect(navigator.mediaCapabilities.decodingInfo).toHaveBeenCalledWith({
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
    });
  }
);

it("returns conservative structured evidence when MSE rejects the source", async () => {
  vi.mocked(MediaSource.isTypeSupported).mockReturnValue(false);
  await expect(determineVideoPlaybackCapability(matrix)).resolves.toMatchObject(
    {
      content_type: matrix.content_type,
      media_source: false,
      supported: false,
      smooth: false,
    }
  );
  expect(navigator.mediaCapabilities.decodingInfo).not.toHaveBeenCalled();
});

it("does not claim HDR direct play without a high-dynamic-range display", async () => {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: false }))
  );
  await expect(determineVideoPlaybackCapability(matrix)).resolves.toMatchObject(
    {
      supported: true,
      smooth: true,
      hdr_display: false,
    }
  );
});

it("returns null when the server cannot derive a remux capability request", async () => {
  await expect(determineVideoPlaybackCapability(null)).resolves.toBeNull();
});
