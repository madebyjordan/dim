import { buildPlaybackManifestUrl, effectiveTrackIndex } from "./QualitySwitch";

it("builds a one-video manifest request for an atomic video replacement", () => {
  const url = buildPlaybackManifestUrl({
    audioId: "audio",
    gid: "session",
    replaceVideo: true,
    videoId: "video-720",
  });
  const parsed = new URL(url, "http://dim.test");

  expect(parsed.searchParams.get("includes")).toBe("video-720,audio");
  expect(parsed.searchParams.get("replace_video")).toBe("true");
});

it("confirms quality by the effective DASH adaptation id, not dimensions", () => {
  const tracks = [
    { set_id: 0, label: "1080p direct" },
    { set_id: 1, label: "1080p transcode" },
  ];

  expect(effectiveTrackIndex(tracks, { id: 1 })).toBe(1);
  expect(effectiveTrackIndex(tracks, { id: 7 })).toBe(-1);
});
