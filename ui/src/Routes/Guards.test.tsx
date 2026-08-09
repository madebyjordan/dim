import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";

import NotAuthedOnlyRoute from "./NotAuthedOnly";
import PrivateRoute from "./Private";

type TestState = {
  auth: { token: string | null; admin_exists: boolean | null };
  user: { fetched: boolean; error: string | null };
};

const dispatch = vi.fn();
let state: TestState;

vi.mock("../hooks/store", () => ({
  useAppDispatch: () => dispatch,
  useAppSelector: <T,>(selector: (current: TestState) => T) => selector(state),
}));

vi.mock("../actions/auth.js", () => ({
  checkAdminExists: () => ({ type: "check-admin" }),
}));

vi.mock("../actions/user.js", () => ({
  fetchUser: () => ({ type: "fetch-user" }),
}));

const renderPrivateRoute = () =>
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route element={<PrivateRoute />}>
          <Route index element={<div>private content</div>} />
        </Route>
        <Route path="/login" element={<div>login screen</div>} />
        <Route path="/register" element={<div>registration screen</div>} />
      </Routes>
    </MemoryRouter>
  );

describe("authentication route guards", () => {
  beforeEach(() => {
    dispatch.mockClear();
    document.cookie = "token=;expires=Thu, 01 Jan 1970 00:00:00 GMT;path=/";
    state = {
      auth: { token: null, admin_exists: null },
      user: { fetched: false, error: null },
    };
  });

  it("renders authenticated routes once the user is loaded", () => {
    document.cookie = "token=session;path=/";
    state.auth.token = "session";
    state.user.fetched = true;

    renderPrivateRoute();

    expect(screen.getByText("private content")).toBeInTheDocument();
    expect(dispatch).toHaveBeenCalledWith({ type: "fetch-user" });
  });

  it("redirects signed-out installations with an admin to login", () => {
    state.auth.admin_exists = true;

    renderPrivateRoute();

    expect(screen.getByText("login screen")).toBeInTheDocument();
    expect(dispatch).not.toHaveBeenCalledWith({ type: "fetch-user" });
  });

  it("redirects first-run installations to registration", () => {
    state.auth.admin_exists = false;

    renderPrivateRoute();

    expect(screen.getByText("registration screen")).toBeInTheDocument();
  });

  it("keeps authenticated users out of public auth screens", () => {
    state.auth.token = "session";

    render(
      <MemoryRouter initialEntries={["/login"]}>
        <Routes>
          <Route element={<NotAuthedOnlyRoute />}>
            <Route path="/login" element={<div>login screen</div>} />
          </Route>
          <Route path="/" element={<div>home screen</div>} />
        </Routes>
      </MemoryRouter>
    );

    expect(screen.getByText("home screen")).toBeInTheDocument();
  });
});
