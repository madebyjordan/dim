import {
  fetchLibraryScanStatus,
  newLibrary,
  rescanLibrary,
  wsScanFailed,
} from "./library";
import {
  ADD_LIBRARY,
  NEW_LIBRARY_ERR,
  NEW_LIBRARY_OK,
  NEW_LIBRARY_START,
  SCAN_FAILED,
  SCAN_CANCELLED,
  SCAN_START,
  SCAN_STOP,
} from "./types";

describe("new library creation", () => {
  const data = {
    name: "Movies",
    locations: ["/Users/test/Movies"],
    media_type: "movie",
  };

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("adds the created library immediately after the server accepts it", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: async () => ({ id: 42, scan_status: "scanning" }),
    });
    const dispatch = vi.fn();

    const result = await newLibrary(data)(dispatch, () => ({
      auth: { token: "owner-token" },
    }));

    expect(result).toEqual({ ok: true, id: 42 });
    expect(dispatch).toHaveBeenCalledWith({ type: NEW_LIBRARY_START });
    expect(dispatch).toHaveBeenCalledWith({ type: NEW_LIBRARY_OK });
    expect(dispatch).toHaveBeenCalledWith({
      type: ADD_LIBRARY,
      payload: {
        id: 42,
        name: "Movies",
        media_type: "movie",
        locations: ["/Users/test/Movies"],
        hidden: false,
      },
    });
    expect(dispatch).toHaveBeenCalledWith({ type: SCAN_START, id: 42 });
  });

  it("returns a controlled server error without adding a library", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      text: async () => "Dim does not have permission to read that folder.",
    });
    const dispatch = vi.fn();

    const result = await newLibrary(data)(dispatch, () => ({
      auth: { token: "owner-token" },
    }));

    expect(result).toEqual({
      ok: false,
      error: "Dim does not have permission to read that folder.",
    });
    expect(dispatch).toHaveBeenCalledWith({
      type: NEW_LIBRARY_ERR,
      payload: "Dim does not have permission to read that folder.",
    });
    expect(dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: ADD_LIBRARY })
    );
  });

  it("stops progress and notifies the user when scanning fails", async () => {
    const dispatch = vi.fn();

    await wsScanFailed(42)(dispatch);

    expect(dispatch).toHaveBeenCalledWith({ type: SCAN_FAILED, id: 42 });
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "notifications/addNotification",
        payload: {
          msg: "The library scan failed. Check that its folders are still readable.",
        },
      })
    );
  });

  it.each([
    ["scanning", SCAN_START],
    ["complete", SCAN_STOP],
    ["failed", SCAN_FAILED],
    ["cancelled", SCAN_CANCELLED],
  ])("hydrates the %s scan state", async (status, type) => {
    const payload = {
      status,
      stage: status === "scanning" ? "matching" : status,
      discovered: 12,
      processed: 10,
      committed: 7,
      skipped: 2,
      failed: 1,
      elapsed_seconds: 30,
      seconds_since_progress: 1,
    };
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: async () => payload,
    });
    const dispatch = vi.fn();

    await fetchLibraryScanStatus(42)(dispatch, () => ({
      auth: { token: "owner-token" },
    }));

    expect(dispatch).toHaveBeenCalledWith({ type, id: 42, payload });
  });

  it("converges on terminal durable truth after a missed WebSocket event", async () => {
    const payload = {
      status: "failed",
      stage: "failed",
      discovered: 4,
      processed: 4,
      committed: 0,
      skipped: 3,
      failed: 1,
      elapsed_seconds: 90,
      seconds_since_progress: 2,
      error_summary: "Dim restarted before this scan finished; retry the scan",
    };
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: async () => payload,
    });
    const dispatch = vi.fn();

    await fetchLibraryScanStatus(42)(dispatch, () => ({
      auth: { token: "owner-token" },
    }));

    expect(dispatch).toHaveBeenCalledWith({
      type: SCAN_FAILED,
      id: 42,
      payload,
    });
  });

  it("starts a manual library rescan", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({ ok: true });
    const dispatch = vi.fn();

    const result = await rescanLibrary(42)(dispatch, () => ({
      auth: { token: "owner-token" },
    }));

    expect(result).toEqual({ ok: true });
    expect(fetch).toHaveBeenCalledWith("/api/v1/library/42/scan", {
      method: "POST",
      headers: { authorization: "owner-token" },
    });
    expect(dispatch).toHaveBeenCalledWith({ type: SCAN_START, id: 42 });
  });
});
