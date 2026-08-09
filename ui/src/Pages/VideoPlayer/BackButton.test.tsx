import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import BackButton from "./BackButton";
import { createPlaybackState } from "./Navigation";

const mockNavigate = vi.fn();
let mockLocation: { state?: { from?: string } } = {};

let mockVideo: { mediaID?: number | null; libraryID?: number | null } = {};

vi.mock("react-router", () => ({
  useLocation: () => mockLocation,
  useNavigate: () => mockNavigate,
}));

vi.mock("../../hooks/store", () => ({
  useAppSelector: (selector: (state: { video: typeof mockVideo }) => unknown) =>
    selector({ video: mockVideo }),
}));

vi.mock("../../assets/Icons/ArrowLeft", () => ({ default: () => null }));

describe("player back navigation", () => {
  beforeEach(() => {
    mockNavigate.mockClear();
    mockLocation = {};
    mockVideo = {};
  });

  it("returns through in-app history when playback has a reliable origin", () => {
    mockLocation = { state: { from: "/media/42" } };
    mockVideo = { mediaID: 42, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockNavigate).toHaveBeenCalledWith(-1);
  });

  it("falls back to the relevant media page without an in-app origin", () => {
    mockVideo = { mediaID: 42, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockNavigate).toHaveBeenCalledWith("/media/42", { replace: true });
  });

  it("falls back to the relevant library while media is unavailable", () => {
    mockVideo = { mediaID: null, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockNavigate).toHaveBeenCalledWith("/library/7", { replace: true });
  });
});

describe("player origin tracking", () => {
  it("records the full route used to enter playback", () => {
    expect(
      createPlaybackState({
        pathname: "/media/42",
        search: "?tab=episodes",
        hash: "#season-2",
      })
    ).toEqual({ from: "/media/42?tab=episodes#season-2" });
  });

  it("preserves the original route across player replacements", () => {
    expect(
      createPlaybackState({
        pathname: "/play/100",
        state: { from: "/media/42" },
      })
    ).toEqual({ from: "/media/42" });
  });
});
