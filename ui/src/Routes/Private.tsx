import { useEffect } from "react";
import { Navigate, Outlet, useLocation } from "react-router";

import { checkAdminExists } from "../actions/auth.js";
import { fetchUser } from "../actions/user.js";
import { useAppDispatch, useAppSelector } from "../hooks/store";
import { getAuthTokenCookie } from "./SessionControllers";
import { apiRequest } from "../api/transport";
import { clearPlaybackSession, getPlaybackSession } from "../storage";

function PrivateRoute() {
  const dispatch = useAppDispatch();
  const location = useLocation();
  const token = useAppSelector((state) => state.auth.token);
  const adminExists = useAppSelector((state) => state.auth.admin_exists);
  const userFetched = useAppSelector((state) => state.user.fetched);
  const userError = useAppSelector((state) => state.user.error);
  const tokenInCookie = getAuthTokenCookie();

  // clears any remaining video streams
  useEffect(() => {
    if (location.pathname.includes("/play/")) return;

    const GID = getPlaybackSession();

    if (!GID) return;

    (async () => {
      await apiRequest(`stream/${GID}/state/kill`, {
        method: "DELETE",
        token,
      }).catch(() => undefined);
      clearPlaybackSession();
    })();
  }, [location.pathname, token]);

  useEffect(() => {
    dispatch(checkAdminExists());
  }, [dispatch]);

  useEffect(() => {
    if (token) dispatch(fetchUser());
  }, [dispatch, token]);

  if (!token && !tokenInCookie) {
    if (adminExists === true) return <Navigate to="/login" replace />;
    if (adminExists === false) return <Navigate to="/register" replace />;
    return <div className="appLoad">Checking Dim setup…</div>;
  }

  if (userError) {
    return (
      <div className="appLoad error">
        <h2>Unable to load your session</h2>
        <p>Dim may be unavailable, or your session may have expired.</p>
        <button onClick={() => window.location.reload()}>Try again</button>
      </div>
    );
  }

  return userFetched && token ? (
    <Outlet />
  ) : (
    <div className="appLoad">Loading your account…</div>
  );
}

export default PrivateRoute;
