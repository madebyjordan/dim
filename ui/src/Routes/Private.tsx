import { useEffect } from "react";
import { Navigate, Outlet, useLocation } from "react-router";

import { checkAdminExists } from "../actions/auth.js";
import { fetchUser } from "../actions/user.js";
import { useAppDispatch, useAppSelector } from "../hooks/store";
import { getAuthTokenCookie } from "./SessionControllers";

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

    const GID = sessionStorage.getItem("GID");

    if (!GID) return;

    (async () => {
      await fetch(`/api/v1/stream/${GID}/state/kill`, {
        method: "DELETE",
        headers: { Authorization: token },
      });
      sessionStorage.clear();
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
    return null;
  }

  return userFetched && !userError && token ? <Outlet /> : null;
}

export default PrivateRoute;
