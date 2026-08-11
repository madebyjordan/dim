import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getPlaybackSession,
  reclaimPlaybackSession,
  setPlaybackSession,
  terminatePlaybackSession,
} from "./storage";

describe("playback session ownership", () => {
  beforeEach(() => sessionStorage.clear());

  it("reclaims the prior tab session before replacement", async () => {
    setPlaybackSession("prior");
    const remove = vi.fn().mockResolvedValue(undefined);

    await reclaimPlaybackSession(remove);

    expect(remove).toHaveBeenCalledWith("prior");
    expect(getPlaybackSession()).toBeNull();
  });

  it("retains ownership when the server did not acknowledge removal", async () => {
    setPlaybackSession("prior");
    const remove = vi.fn().mockRejectedValue({ status: "FETCH_ERROR" });

    await expect(terminatePlaybackSession("prior", remove)).rejects.toEqual({
      status: "FETCH_ERROR",
    });
    expect(getPlaybackSession()).toBe("prior");
  });

  it("treats an already-removed server session as reclaimed", async () => {
    setPlaybackSession("prior");

    await terminatePlaybackSession("prior", async () => {
      throw { status: 404 };
    });

    expect(getPlaybackSession()).toBeNull();
  });

  it("does not clear a newer session after an older delete completes", async () => {
    setPlaybackSession("prior");
    let acknowledge: (() => void) | undefined;
    const removal = terminatePlaybackSession(
      "prior",
      () =>
        new Promise<void>((resolve) => {
          acknowledge = resolve;
        })
    );

    setPlaybackSession("current");
    acknowledge?.();
    await removal;

    expect(getPlaybackSession()).toBe("current");
  });
});
