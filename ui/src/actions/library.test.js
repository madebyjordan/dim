import { newLibrary, wsScanFailed } from "./library";
import {
  ADD_LIBRARY,
  NEW_LIBRARY_ERR,
  NEW_LIBRARY_OK,
  NEW_LIBRARY_START,
  SCAN_STOP,
} from "./types";

describe("new library creation", () => {
  const data = {
    name: "Movies",
    locations: ["/Users/test/Movies"],
    media_type: "movie",
  };

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it("adds the created library immediately after the server accepts it", async () => {
    jest.spyOn(global, "fetch").mockResolvedValue({
      ok: true,
      json: async () => ({ id: 42, scan_status: "started" }),
    });
    const dispatch = jest.fn();

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
  });

  it("returns a controlled server error without adding a library", async () => {
    jest.spyOn(global, "fetch").mockResolvedValue({
      ok: false,
      text: async () => "Dim does not have permission to read that folder.",
    });
    const dispatch = jest.fn();

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
    const dispatch = jest.fn();

    await wsScanFailed(42)(dispatch);

    expect(dispatch).toHaveBeenCalledWith({ type: SCAN_STOP, id: 42 });
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "notifications/addNotification",
        payload: {
          msg: "The library scan failed. Check that its folders are still readable.",
        },
      })
    );
  });
});
