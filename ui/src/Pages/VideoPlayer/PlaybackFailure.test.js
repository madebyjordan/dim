import {
  consumeRetryPosition,
  PLAYBACK_ERROR_MESSAGE,
  RETRY_POSITION_KEY,
  stopFailedPlayback,
} from "./PlaybackFailure";

describe("playback failure handling", () => {
  it("keeps FFmpeg diagnostics in logs and returns a controlled UI error", async () => {
    const request = vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ errors: ["raw ffmpeg output"] }),
      })
      .mockResolvedValueOnce({ ok: true });
    const logger = vi.fn();

    const error = await stopFailedPlayback({
      details: "segment unavailable",
      gid: "stream-group",
      logger,
      request,
      token: "token",
    });

    expect(error).toEqual({ msg: PLAYBACK_ERROR_MESSAGE });
    expect(error).not.toHaveProperty("errors");
    expect(logger).toHaveBeenCalledWith(
      "[VIDEO] playback failed",
      expect.objectContaining({ ffmpegDiagnostics: ["raw ffmpeg output"] })
    );
    expect(request).toHaveBeenLastCalledWith(
      "/api/v1/stream/stream-group/state/kill",
      {
        method: "DELETE",
        headers: { Authorization: "token" },
      }
    );
  });

  it("still stops the stream when diagnostics cannot be fetched", async () => {
    const request = vi
      .fn()
      .mockRejectedValueOnce(new Error("diagnostics unavailable"))
      .mockResolvedValueOnce({ ok: true });

    await stopFailedPlayback({
      gid: "stream-group",
      logger: vi.fn(),
      request,
      token: "token",
    });

    expect(request).toHaveBeenLastCalledWith(
      "/api/v1/stream/stream-group/state/kill",
      expect.objectContaining({ method: "DELETE" })
    );
  });

  it("restores and consumes the retry position", () => {
    const values = new Map([[RETRY_POSITION_KEY, "45"]]);
    const storage = {
      getItem: (key) => values.get(key),
      removeItem: (key) => values.delete(key),
    };

    expect(consumeRetryPosition(storage)).toBe(45);
    expect(values.has(RETRY_POSITION_KEY)).toBe(false);
  });
});
