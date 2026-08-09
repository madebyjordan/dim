import { createReducer } from "@reduxjs/toolkit";

import {
  FETCH_USER_SETTINGS_START,
  FETCH_USER_SETTINGS_OK,
  FETCH_USER_SETTINGS_ERR,
  FETCH_GLOBAL_SETTINGS_START,
  FETCH_GLOBAL_SETTINGS_OK,
  FETCH_GLOBAL_SETTINGS_ERR,
  UPDATE_GLOBAL_SETTINGS,
  UPDATE_USER_SETTINGS,
} from "../actions/types";

const requestState = () => ({
  fetching: false,
  fetched: false,
  error: null,
  data: {},
});

/**
 * @typedef {object} SettingsData
 * @property {string} [theme]
 * @property {string} [version]
 * @property {boolean} [show_hovercards]
 * @property {boolean} [show_card_names]
 * @property {boolean} [enable_autoplay]
 * @property {boolean} [enable_hwaccel]
 * @property {boolean} [is_sidebar_compact]
 * @property {boolean} [disable_auth]
 * @property {boolean} [verbose]
 * @property {number} [port]
 * @property {string} [cache_dir]
 * @property {string} [metadata_dir]
 * @property {unknown} [default_video_quality]
 */

/** @type {{globalSettings: ReturnType<typeof requestState> & {data: SettingsData}, userSettings: ReturnType<typeof requestState> & {data: SettingsData}}} */
const initialState = {
  globalSettings: requestState(),
  userSettings: requestState(),
};

export default createReducer(initialState, (builder) => {
  builder
    .addCase(FETCH_USER_SETTINGS_START, (state) => {
      state.userSettings = {
        fetching: true,
        fetched: false,
        error: null,
        data: {},
      };
    })
    .addCase(FETCH_USER_SETTINGS_OK, (state, action) => {
      state.userSettings.fetching = false;
      state.userSettings.fetched = true;
      state.userSettings.data = action.payload;
    })
    .addCase(FETCH_USER_SETTINGS_ERR, (state, action) => {
      state.userSettings.fetching = false;
      state.userSettings.fetched = true;
      state.userSettings.error = action.payload;
    })
    .addCase(UPDATE_USER_SETTINGS, (state, action) => {
      state.userSettings.data = action.payload;
    })
    .addCase(FETCH_GLOBAL_SETTINGS_START, (state) => {
      state.globalSettings = {
        fetching: true,
        fetched: false,
        error: null,
        data: {},
      };
    })
    .addCase(FETCH_GLOBAL_SETTINGS_OK, (state, action) => {
      state.globalSettings.fetching = false;
      state.globalSettings.fetched = true;
      state.globalSettings.data = action.payload;
    })
    .addCase(FETCH_GLOBAL_SETTINGS_ERR, (state, action) => {
      state.globalSettings.fetching = false;
      state.globalSettings.fetched = true;
      state.globalSettings.error = action.payload;
    })
    .addCase(UPDATE_GLOBAL_SETTINGS, (state, action) => {
      state.globalSettings.data = action.payload;
    });
});
