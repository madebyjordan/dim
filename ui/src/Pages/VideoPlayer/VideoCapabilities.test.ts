import {
  supportsVerifiedAv1Playback,
  VERIFIED_AV1_CONTENT_TYPE,
} from "./VideoCapabilities";

const chrome151 =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) " +
  "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

const setUserAgent = (value: string) =>
  Object.defineProperty(navigator, "userAgent", { configurable: true, value });

beforeEach(() => {
  setUserAgent(chrome151);
  vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue(
    "probably"
  );
  vi.stubGlobal("MediaSource", { isTypeSupported: vi.fn(() => true) });
  Object.defineProperty(navigator, "mediaCapabilities", {
    configurable: true,
    value: {
      decodingInfo: vi
        .fn()
        .mockResolvedValue({ supported: true, smooth: true }),
    },
  });
});

afterEach(() => vi.restoreAllMocks());

it("reports only the independently verified Chromium AV1 envelope", async () => {
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(true);
  expect(navigator.mediaCapabilities.decodingInfo).toHaveBeenCalledWith({
    type: "media-source",
    video: {
      contentType: VERIFIED_AV1_CONTENT_TYPE,
      width: 1920,
      height: 1080,
      bitrate: 6_300_000,
      framerate: 24,
    },
  });
});

it("keeps unknown browsers and incomplete evidence on the conservative fallback", async () => {
  setUserAgent("Mozilla/5.0 Firefox/142.0");
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(false);

  setUserAgent(chrome151.replace("Chrome/151", "Chrome/152"));
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(false);

  setUserAgent(
    chrome151.replace("Macintosh; Intel Mac OS X 10_15_7", "Windows NT 10.0")
  );
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(false);

  setUserAgent(
    chrome151.replace("Chrome/151.0.0.0", "Chrome/151.0.0.0 Edg/151.0")
  );
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(false);

  setUserAgent(chrome151);
  vi.mocked(MediaSource.isTypeSupported).mockReturnValue(false);
  await expect(supportsVerifiedAv1Playback()).resolves.toBe(false);
});
