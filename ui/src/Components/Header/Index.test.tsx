import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";

import Header from "./Index";

const dispatch = vi.fn();
const state = {
  user: {
    fetching: false,
    fetched: true,
    error: null,
    info: { username: "Alex", picture: "", roles: ["owner"] },
  },
  library: {
    fetch_libraries: {
      fetched: true,
      items: [
        { id: 1, name: "Movies", media_type: "movie" },
        { id: 2, name: "Shows", media_type: "tv" },
      ],
    },
    scanning: [1],
    scan_progress: { 1: { discovered: 53, processed: 25 } },
  },
};

vi.mock("hooks/store", () => ({
  useAppDispatch: () => dispatch,
  useAppSelector: <T,>(selector: (current: typeof state) => T) =>
    selector(state),
}));

vi.mock("hooks/ws", () => ({ default: () => null }));
vi.mock("actions/settings.js", () => ({
  fetchGlobalSettings: () => ({ type: "global-settings" }),
  fetchUserSettings: () => ({ type: "user-settings" }),
}));
vi.mock("actions/library.js", () => ({
  fetchLibraries: () => ({ type: "libraries" }),
  fetchLibraryScanStatus: (id: number) => ({ type: "scan-status", id }),
  handleWsDelLibrary: vi.fn(),
  handleWsNewLibrary: vi.fn(),
  wsScanCancelled: vi.fn(),
  wsScanFailed: vi.fn(),
  wsScanStart: vi.fn(),
  wsScanStop: vi.fn(),
}));
vi.mock("../../Modals/NewLibrary/Index", () => ({
  default: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("../Sidebar/Search", () => ({
  default: () => <button aria-label="Search" />,
}));
vi.mock("../Sidebar/Profile/LogoutBtn", () => ({
  default: () => <button>Logout</button>,
}));

describe("Eclipse header", () => {
  beforeEach(() => dispatch.mockClear());

  it("maps navigation and real scanner progress onto the existing libraries", () => {
    render(
      <MemoryRouter initialEntries={["/library/1"]}>
        <Header />
      </MemoryRouter>
    );

    expect(screen.getByRole("link", { name: "Movies" })).toHaveAttribute(
      "href",
      "/library/1"
    );
    expect(screen.getByRole("link", { name: "Movies" })).toHaveClass("active");
    expect(screen.getByRole("link", { name: "Shows" })).toHaveAttribute(
      "href",
      "/library/2"
    );
    expect(screen.getByText("Watchlist")).toHaveAttribute(
      "aria-disabled",
      "true"
    );
    expect(screen.getByRole("button", { name: "Add" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Scanning Movies 25/53"
    );
  });

  it("keeps dashboard and preferences available from profile access", () => {
    render(
      <MemoryRouter>
        <Header />
      </MemoryRouter>
    );

    userEvent.click(screen.getByRole("button", { name: "Open profile menu" }));

    expect(screen.getByRole("link", { name: "Dashboard" })).toHaveAttribute(
      "href",
      "/"
    );
    expect(screen.getByRole("link", { name: "Preferences" })).toHaveAttribute(
      "href",
      "/preferences"
    );
  });
});
