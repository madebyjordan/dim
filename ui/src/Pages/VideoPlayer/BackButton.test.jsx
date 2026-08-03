import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import BackButton from "./BackButton";
import { createPlaybackState } from "./Navigation";

const mockHistory = {
  goBack: jest.fn(),
  replace: jest.fn(),
  location: {},
};

let mockVideo = {};

jest.mock("react-router-dom", () => ({
  useHistory: () => mockHistory,
}));

jest.mock("react-redux", () => ({
  useSelector: (selector) => selector({ video: mockVideo }),
}));

jest.mock("../../assets/Icons/ArrowLeft", () => () => null);

describe("player back navigation", () => {
  beforeEach(() => {
    mockHistory.goBack.mockClear();
    mockHistory.replace.mockClear();
    mockHistory.location = {};
    mockVideo = {};
  });

  it("returns through in-app history when playback has a reliable origin", () => {
    mockHistory.location = { state: { from: "/media/42" } };
    mockVideo = { mediaID: 42, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockHistory.goBack).toHaveBeenCalledTimes(1);
    expect(mockHistory.replace).not.toHaveBeenCalled();
  });

  it("falls back to the relevant media page without an in-app origin", () => {
    mockVideo = { mediaID: 42, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockHistory.goBack).not.toHaveBeenCalled();
    expect(mockHistory.replace).toHaveBeenCalledWith("/media/42");
  });

  it("falls back to the relevant library while media is unavailable", () => {
    mockVideo = { mediaID: null, libraryID: 7 };

    render(<BackButton />);
    userEvent.click(screen.getByRole("button", { name: "Back" }));

    expect(mockHistory.replace).toHaveBeenCalledWith("/library/7");
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
