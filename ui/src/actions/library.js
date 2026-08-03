import {
  FETCH_LIBRARIES_START,
  FETCH_LIBRARIES_OK,
  FETCH_LIBRARIES_ERR,
  FETCH_LIBRARY_INFO,
  FETCH_LIBRARY_MEDIA,
  NEW_LIBRARY_START,
  NEW_LIBRARY_OK,
  NEW_LIBRARY_ERR,
  DEL_LIBRARY_START,
  DEL_LIBRARY_OK,
  DEL_LIBRARY_ERR,
  RM_LIBRARY,
  ADD_LIBRARY,
  SCAN_START,
  SCAN_STOP,
  SCAN_FAILED,
  FETCH_LIBRARY_UNMATCHED_START,
  FETCH_LIBRARY_UNMATCHED_ERR,
  FETCH_LIBRARY_UNMATCHED_OK,
} from "./types";
import { addNotification } from "../slices/notifications";

const responseError = async (response, fallback) => {
  try {
    const message = await response.text();
    return message || fallback;
  } catch (_) {
    return fallback;
  }
};

export const fetchLibraries = () => async (dispatch, getState) => {
  const token = getState().auth.token;

  dispatch({ type: FETCH_LIBRARIES_START });

  try {
    const config = {
      headers: {
        authorization: token,
      },
    };
    const res = await fetch("/api/v1/library", config);

    if (res.status !== 200) {
      return dispatch({
        type: FETCH_LIBRARIES_ERR,
        payload: res.statusText,
      });
    }

    const payload = await res.json();

    dispatch({
      type: FETCH_LIBRARIES_OK,
      payload,
    });
  } catch (err) {
    dispatch({
      type: FETCH_LIBRARIES_ERR,
      payload: err,
    });
  }
};

export const fetchLibraryUnmatched = (id) => async (dispatch, getState) => {
  const token = getState().auth.token;

  dispatch({ type: FETCH_LIBRARY_UNMATCHED_START });

  try {
    const config = {
      headers: {
        authorization: token,
      },
    };

    const res = await fetch(`/api/v1/library/${id}/unmatched`, config);

    if (res.status !== 200) {
      return dispatch({
        type: FETCH_LIBRARY_UNMATCHED_ERR,
        payload: res.statusText,
      });
    }

    const payload = await res.json();

    dispatch({
      type: FETCH_LIBRARY_UNMATCHED_OK,
      payload,
    });
  } catch (err) {
    dispatch({
      type: FETCH_LIBRARY_UNMATCHED_ERR,
      payload: err,
    });
  }
};

export const newLibrary = (data) => async (dispatch, getState) => {
  const token = getState().auth.token;

  dispatch({ type: NEW_LIBRARY_START });

  const options = {
    method: "POST",
    headers: {
      Authorization: token,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(data),
  };

  try {
    const res = await fetch("/api/v1/library", options);

    if (!res.ok) {
      const error = await responseError(
        res,
        "Dim could not create the library."
      );
      dispatch({
        type: NEW_LIBRARY_ERR,
        payload: error,
      });
      return { ok: false, error };
    }

    const payload = await res.json();
    dispatch({ type: NEW_LIBRARY_OK });
    dispatch({
      type: ADD_LIBRARY,
      payload: {
        id: payload.id,
        name: data.name,
        media_type: data.media_type,
        locations: data.locations,
        hidden: false,
      },
    });
    dispatch({ type: SCAN_START, id: payload.id });
    dispatch(
      addNotification({
        msg: "Library created. The initial scan has started.",
      })
    );
    return { ok: true, id: payload.id };
  } catch (err) {
    const error = "Dim could not reach the server to create the library.";
    dispatch({
      type: NEW_LIBRARY_ERR,
      payload: error,
    });
    return { ok: false, error };
  }
};

export const delLibrary = (id) => async (dispatch, getState) => {
  const token = getState().auth.token;

  dispatch({ type: DEL_LIBRARY_START });

  const options = {
    headers: {
      authorization: token,
    },
    method: "DELETE",
  };

  try {
    const res = await fetch(`/api/v1/library/${id}`, options);

    if (res.status !== 204) {
      return dispatch({
        type: DEL_LIBRARY_ERR,
        payload: res.statusText,
      });
    }

    dispatch({ type: DEL_LIBRARY_OK });
  } catch (err) {
    dispatch({
      type: DEL_LIBRARY_ERR,
      payload: err,
    });
  }
};

export const fetchLibraryInfo = () => async (dispatch) => {
  dispatch({ type: FETCH_LIBRARY_INFO });
};

export const fetchLibraryMedia = () => async (dispatch) => {
  dispatch({ type: FETCH_LIBRARY_MEDIA });
};

export const handleWsDelLibrary = (id) => async (dispatch) => {
  dispatch({
    type: RM_LIBRARY,
    id,
  });
};

export const handleWsNewLibrary = (id) => async (dispatch, getState) => {
  const token = getState().auth.token;

  const options = {
    headers: {
      Authorization: token,
    },
  };

  try {
    const res = await fetch(`/api/v1/library/${id}`, options);

    if (res.status !== 200) {
      return;
    }

    const info = await res.json();

    dispatch({
      type: ADD_LIBRARY,
      payload: info,
    });
  } catch (err) {}
};

export const wsScanStart = (id) => async (dispatch) => {
  dispatch({
    type: SCAN_START,
    id,
  });
};

export const wsScanStop = (id) => async (dispatch) => {
  dispatch({
    type: SCAN_STOP,
    id,
  });
};

export const wsScanFailed = (id) => async (dispatch) => {
  dispatch({
    type: SCAN_FAILED,
    id,
  });
  dispatch(
    addNotification({
      msg: "The library scan failed. Check that its folders are still readable.",
    })
  );
};

export const fetchLibraryScanStatus = (id) => async (dispatch, getState) => {
  const token = getState().auth.token;

  try {
    const res = await fetch(`/api/v1/library/${id}/scan`, {
      headers: { authorization: token },
    });

    if (!res.ok) return;

    const { status } = await res.json();
    const type = {
      scanning: SCAN_START,
      complete: SCAN_STOP,
      failed: SCAN_FAILED,
    }[status];

    if (type) dispatch({ type, id });
  } catch (_) {}
};

export const retryLibraryScan = (id) => async (dispatch, getState) => {
  const token = getState().auth.token;

  try {
    const res = await fetch(`/api/v1/library/${id}/scan`, {
      method: "POST",
      headers: { authorization: token },
    });

    if (!res.ok) {
      const error = await responseError(res, "Dim could not restart the scan.");
      dispatch(addNotification({ msg: error }));
      return { ok: false, error };
    }

    dispatch({ type: SCAN_START, id });
    return { ok: true };
  } catch (_) {
    const error = "Dim could not reach the server to restart the scan.";
    dispatch(addNotification({ msg: error }));
    return { ok: false, error };
  }
};
