import { useCallback, useEffect, useRef, useState } from "react";
import { useDispatch, useSelector } from "react-redux";
import { useParams } from "react-router";
import { skipToken } from "@reduxjs/toolkit/query/react";

import Card from "../../Components/Card/Index";
import { fetchLibraryScanStatus, rescanLibrary } from "../../actions/library";
import useWebSocket from "../../hooks/ws";
import Dropdown from "./Dropdown";
import LibraryState from "./LibraryState";
import { useGetLibraryMediaQuery } from "../../api/v1/foundation";

import "./Cards.scss";

function Cards() {
  const { id } = useParams();
  const dispatch = useDispatch();
  const ws = useWebSocket();
  const refreshTimer = useRef();

  const canRescan = useSelector((store) =>
    store.user.info.roles?.includes("owner")
  );
  const libraries = useSelector((store) => store.library.fetch_libraries.items);
  const scanState = useSelector((store) => store.library.scan_status[id]);
  const scanProgress = useSelector((store) => store.library.scan_progress[id]);
  const library = libraries.find((item) => String(item.id) === String(id));

  const [scanStarting, setScanStarting] = useState(false);
  const {
    data: media,
    isError,
    isLoading,
    isFetching,
    refetch,
  } = useGetLibraryMediaQuery(id ?? skipToken);
  const responseTitle = media ? Object.keys(media)[0] : "";
  const cards = media ? Object.values(media)[0] || [] : [];
  const mediaState = isError
    ? "error"
    : (isLoading || isFetching) && !media
    ? "loading"
    : cards.length > 0
    ? "results"
    : "empty";

  const title = library?.name || responseTitle || "Library";

  useEffect(() => {
    dispatch(fetchLibraryScanStatus(id));
  }, [dispatch, id]);

  useEffect(() => {
    if (scanState === "complete") refetch();
  }, [refetch, scanState]);

  useEffect(() => {
    if (scanState !== "scanning") return undefined;

    const statusTimer = setInterval(() => {
      dispatch(fetchLibraryScanStatus(id));
    }, 2000);

    return () => clearInterval(statusTimer);
  }, [dispatch, id, scanState]);

  const handleWS = useCallback(
    ({ data }) => {
      const event = JSON.parse(data);
      if (
        event.type !== "EventNewCard" ||
        String(event.lib_id) !== String(id)
      ) {
        return;
      }

      clearTimeout(refreshTimer.current);
      refreshTimer.current = setTimeout(() => refetch(), 500);
    },
    [id, refetch]
  );

  useEffect(() => {
    if (!ws) return undefined;

    ws.addEventListener("message", handleWS);
    return () => {
      ws.removeEventListener("message", handleWS);
      clearTimeout(refreshTimer.current);
    };
  }, [handleWS, ws]);

  useEffect(() => {
    document.title = `Dim - ${title}`;
  }, [title]);

  const handleRescan = async () => {
    if (scanStarting || scanState === "scanning") return;

    setScanStarting(true);
    await dispatch(rescanLibrary(id));
    setScanStarting(false);
  };

  return (
    <div className="libraryCards">
      <div className="libraryHeader">
        <h2>{title}</h2>
        <div className="actions">
          <Dropdown
            onRescan={handleRescan}
            scanStarting={scanStarting}
            scanning={scanState === "scanning"}
          />
        </div>
      </div>

      <LibraryState
        canRescan={canRescan}
        mediaState={mediaState}
        mediaType={library?.media_type}
        onRescan={handleRescan}
        scanStarting={scanStarting}
        scanState={scanState}
        scanProgress={scanProgress}
      />

      {cards.length > 0 && (
        <div className="cards">
          {cards.map((card, index) => (
            <Card key={card.id || index} data={card} />
          ))}
        </div>
      )}
    </div>
  );
}

export default Cards;
