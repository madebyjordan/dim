import { useLocation, useNavigate } from "react-router";

import ArrowLeftIcon from "../../assets/Icons/ArrowLeft";
import { useAppSelector } from "../../hooks/store";
import { navigateBackFromPlayback } from "./Navigation";

function BackButton() {
  const location = useLocation();
  const navigate = useNavigate();
  const mediaID = useAppSelector((state) => state.video.mediaID);
  const libraryID = useAppSelector((state) => state.video.libraryID);

  const goBack = () => {
    navigateBackFromPlayback(navigate, location, { mediaID, libraryID });
  };

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
