import { useLocation, useNavigate } from "react-router";
import { shallowEqual } from "react-redux";

import { useAppSelector } from "hooks/store";
import { RETRY_POSITION_KEY } from "./PlaybackFailure";
import { navigateBackFromPlayback } from "./Navigation";

function ErrorBox() {
  const location = useLocation();
  const navigate = useNavigate();

  const { video, error } = useAppSelector(
    (store) => ({
      video: store.video,
      error: store.video.error,
    }),
    shallowEqual
  );

  const reloadPlayer = () => {
    sessionStorage.setItem(RETRY_POSITION_KEY, String(video.currentTime));
    window.location.reload();
  };

  const leavePlayer = () => {
    navigateBackFromPlayback(navigate, location, {
      mediaID: video.mediaID,
      libraryID: video.libraryID,
    });
  };

  return (
    <div className="errorBox">
      <h2>Error</h2>
      <div className="separator" />
      <p>{error.msg}</p>
      <div className="options">
        <button onClick={leavePlayer}>Back</button>
        <button onClick={reloadPlayer}>Retry playback</button>
      </div>
    </div>
  );
}

export default ErrorBox;
