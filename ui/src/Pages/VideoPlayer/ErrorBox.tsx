import { useHistory } from "react-router-dom";

import { useAppSelector } from "hooks/store";
import { RETRY_POSITION_KEY } from "./PlaybackFailure";
import { navigateBackFromPlayback } from "./Navigation";

function ErrorBox() {
  const history = useHistory();

  const { video, error } = useAppSelector((store) => ({
    video: store.video,
    error: store.video.error,
  }));

  const reloadPlayer = () => {
    sessionStorage.setItem(RETRY_POSITION_KEY, String(video.currentTime));
    window.location.reload();
  };

  const leavePlayer = () => {
    navigateBackFromPlayback(history, {
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
