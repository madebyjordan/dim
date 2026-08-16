import { Navigate, Outlet } from "react-router";

import { useAppSelector } from "../hooks/store";
import { getAuthTokenCookie } from "./SessionControllers";

function NotAuthedOnlyRoute() {
  const token = useAppSelector((state) => state.auth.token);

  return token || getAuthTokenCookie() ? (
    <Navigate to="/" replace />
  ) : (
    <Outlet />
  );
}

export default NotAuthedOnlyRoute;
