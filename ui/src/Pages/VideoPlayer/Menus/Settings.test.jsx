import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useDispatch, useSelector } from "react-redux";

import { VideoPlayerContext } from "../Context";
import VideoMenuSettings from "./Settings";

vi.mock("react-redux", () => ({
  shallowEqual: vi.fn(),
  useDispatch: vi.fn(),
  useSelector: vi.fn(),
}));

const video = {
  tracks: {
    video: {
      current: 0,
      list: [
        { label: "1080p", set_id: "loaded-video" },
        { label: "720p", set_id: "absent-video" },
      ],
    },
    audio: { current: 0, list: [] },
  },
};

function renderSettings(player, dispatch = vi.fn()) {
  useDispatch.mockReturnValue(dispatch);
  useSelector.mockImplementation((selector) => selector({ video }));

  render(
    <VideoPlayerContext.Provider value={{ player }}>
      <VideoMenuSettings />
    </VideoPlayerContext.Provider>
  );

  return dispatch;
}

it("does not report success when a planned video track is absent from the manifest", () => {
  const player = {
    getTracksFor: vi.fn().mockReturnValue([{ id: "loaded-video" }]),
    setCurrentTrack: vi.fn(),
  };
  const dispatch = renderSettings(player);

  userEvent.click(screen.getByText("Video tracks"));
  userEvent.click(screen.getByText("720p"));

  expect(player.setCurrentTrack).not.toHaveBeenCalled();
  expect(dispatch).not.toHaveBeenCalled();
  expect(screen.getByText("1080p").parentElement).toHaveClass("active");
  expect(screen.getByText("720p").parentElement).not.toHaveClass("active");
});

it("updates the selected video track after dash.js confirms it is loaded", () => {
  const loadedTrack = { id: "absent-video" };
  const player = {
    getTracksFor: vi.fn().mockReturnValue([loadedTrack]),
    setCurrentTrack: vi.fn(),
  };
  const dispatch = renderSettings(player);

  userEvent.click(screen.getByText("Video tracks"));
  userEvent.click(screen.getByText("720p"));

  expect(player.setCurrentTrack).toHaveBeenCalledWith(loadedTrack);
  expect(dispatch).toHaveBeenCalledTimes(1);
});
