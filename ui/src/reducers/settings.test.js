import {
  FETCH_GLOBAL_SETTINGS_ERR,
  FETCH_GLOBAL_SETTINGS_START,
  UPDATE_USER_SETTINGS,
} from "../actions/types";
import settingsReducer from "./settings";

describe("settings reducer", () => {
  it("records global settings failures instead of restarting the request", () => {
    const loading = settingsReducer(undefined, {
      type: FETCH_GLOBAL_SETTINGS_START,
    });
    const failed = settingsReducer(loading, {
      type: FETCH_GLOBAL_SETTINGS_ERR,
      payload: "offline",
    });

    expect(failed.globalSettings).toEqual({
      fetching: false,
      fetched: true,
      error: "offline",
      data: {},
    });
  });

  it("uses Immer updates without changing the previous state", () => {
    const initial = settingsReducer(undefined, { type: "init" });
    const updated = settingsReducer(initial, {
      type: UPDATE_USER_SETTINGS,
      payload: { show_card_names: true },
    });

    expect(updated.userSettings.data).toEqual({ show_card_names: true });
    expect(initial.userSettings.data).toEqual({});
  });
});
