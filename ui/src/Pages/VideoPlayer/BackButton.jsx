import { useCallback } from "react";
import { useSelector } from "react-redux";
import { useHistory } from "react-router-dom";

import ArrowLeftIcon from "../../assets/Icons/ArrowLeft";
import { navigateBackFromPlayback } from "./Navigation";

function BackButton() {
  const history = useHistory();
  const { mediaID, libraryID } = useSelector((store) => store.video);

  const goBack = useCallback(() => {
    navigateBackFromPlayback(history, { mediaID, libraryID });
  }, [history, libraryID, mediaID]);

  return (
    <button
      type="button"
      className="videoBack"
      aria-label="Back"
      title="Leave playback"
      onClick={goBack}
    >
      <ArrowLeftIcon />
      <span>Back</span>
    </button>
  );
}

export default BackButton;
