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
        { label: "Direct Play (1080p)", set_id: "loaded-video" },
        { label: "720p · 5 Mb/s", set_id: "absent-video" },
        { label: "480p · 1 Mb/s", set_id: "low-video" },
      ],
    },
    audio: { current: 0, list: [] },
  },
};

function renderSettings(
  player,
  dispatch = vi.fn(),
  changeVideoQuality = vi.fn()
) {
  useDispatch.mockReturnValue(dispatch);
  useSelector.mockImplementation((selector) => selector({ video }));

  render(
    <VideoPlayerContext.Provider value={{ changeVideoQuality, player }}>
      <VideoMenuSettings />
    </VideoPlayerContext.Provider>
  );

  return { changeVideoQuality, dispatch };
}

it("requests lazy activation without reporting success immediately", () => {
  const player = {
    getTracksFor: vi.fn().mockReturnValue([{ id: "loaded-video" }]),
    setCurrentTrack: vi.fn(),
  };
  const { changeVideoQuality, dispatch } = renderSettings(player);

  userEvent.click(screen.getByText("Video tracks"));
  userEvent.click(screen.getByText("720p · 5 Mb/s"));

  expect(player.setCurrentTrack).not.toHaveBeenCalled();
  expect(changeVideoQuality).toHaveBeenCalledWith(1);
  expect(dispatch).not.toHaveBeenCalled();
  expect(screen.getByText("Direct Play (1080p)").parentElement).toHaveClass(
    "active"
  );
  expect(screen.getByText("720p · 5 Mb/s").parentElement).not.toHaveClass(
    "active"
  );
  expect(screen.getByText("480p · 1 Mb/s")).toBeInTheDocument();
  expect(screen.queryByText("1080p · 10 Mb/s")).not.toBeInTheDocument();
});

it("keeps direct dash.js track switching for audio", () => {
  video.tracks.audio.list = [
    { label: "English", set_id: "loaded-audio" },
    { label: "French", set_id: "french-audio" },
  ];
  const player = {
    getTracksFor: vi
      .fn()
      .mockReturnValue([{ id: "loaded-audio" }, { id: "french-audio" }]),
    setCurrentTrack: vi.fn(),
  };
  const { dispatch } = renderSettings(player);

  userEvent.click(screen.getByText("Audio tracks"));
  userEvent.click(screen.getByText("French"));

  expect(player.setCurrentTrack).toHaveBeenCalledWith({ id: "french-audio" });
  expect(dispatch).toHaveBeenCalledTimes(1);
});
