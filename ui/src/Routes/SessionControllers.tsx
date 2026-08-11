import { useEffect } from "react";
import { useLocation } from "react-router";

import { updateAuthToken } from "../actions/auth.js";
import { logout } from "../actions/auth.js";
import { SESSION_EXPIRED_EVENT } from "../api/transport";
import { useAppDispatch, useAppSelector } from "../hooks/store";

export const getAuthTokenCookie = () =>
  window.sessionStorage.getItem("dim-bearer-token") ?? undefined;

export const AuthSessionController = () => {
  const dispatch = useAppDispatch();
  const token = useAppSelector((state) => state.auth.token);
  const loggedIn = useAppSelector((state) => state.auth.login.logged_in);
  const loginError = useAppSelector((state) => state.auth.login.error);
  const storedToken = getAuthTokenCookie();

  useEffect(() => {
    if (storedToken && !token) dispatch(updateAuthToken(storedToken));
  }, [storedToken, dispatch, token]);

  useEffect(() => {
    if (!loggedIn || !token || loginError || storedToken === token) return;
    window.sessionStorage.setItem("dim-bearer-token", token);
  }, [storedToken, loggedIn, loginError, token]);

  useEffect(() => {
    const expire = () => {
      dispatch(logout());
      if (!window.location.pathname.startsWith("/login")) {
        window.location.replace("/login?reason=expired");
      }
    };
    window.addEventListener(SESSION_EXPIRED_EVENT, expire);
    return () => window.removeEventListener(SESSION_EXPIRED_EVENT, expire);
  }, [dispatch]);

  return null;
};

export const CrossTabAuthController = () => {
  const { pathname } = useLocation();

  useEffect(() => {
    if (!("BroadcastChannel" in window)) return;

    const channel = new BroadcastChannel("dim");
    channel.onmessage = ({ data }: MessageEvent<unknown>) => {
      if (document.hasFocus()) return;

      if (data === "login" && ["/login", "/register"].includes(pathname)) {
        window.location.replace("/");
      }

      if (data === "logout" && !["/login", "/register"].includes(pathname)) {
        window.location.replace("/login");
      }
    };

    return () => channel.close();
  }, [pathname]);

  return null;
};

export const ScrollToTop = () => {
  const { pathname } = useLocation();

  useEffect(() => {
    window.scrollTo(0, 0);
  }, [pathname]);

  return null;
};
