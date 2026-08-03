import { useCallback, useEffect, useRef, useState } from "react";
import { useDispatch, useSelector } from "react-redux";
import { useParams } from "react-router";

import Card from "../../Components/Card/Index";
import {
  fetchLibraryScanStatus,
  retryLibraryScan,
} from "../../actions/library";
import useWebSocket from "../../hooks/ws";
import Dropdown from "./Dropdown";
import LibraryState from "./LibraryState";

import "./Cards.scss";

function Cards() {
  const { id } = useParams();
  const dispatch = useDispatch();
  const ws = useWebSocket();
  const refreshTimer = useRef();

  const auth = useSelector((store) => store.auth);
  const libraries = useSelector((store) => store.library.fetch_libraries.items);
  const scanState = useSelector((store) => store.library.scan_status[id]);
  const library = libraries.find((item) => String(item.id) === String(id));

  const [cards, setCards] = useState([]);
  const [mediaState, setMediaState] = useState("loading");
  const [responseTitle, setResponseTitle] = useState("");
  const [retrying, setRetrying] = useState(false);

  const title = library?.name || responseTitle || "Library";

  const fetchCards = useCallback(
    async (showLoading = false) => {
      if (showLoading) setMediaState("loading");

      try {
        const res = await fetch(`/api/v1/library/${id}/media`, {
          headers: { authorization: auth.token },
        });

        if (res.status === 404) {
          setCards([]);
          setMediaState("empty");
          return;
        }

        if (!res.ok) {
          setMediaState("error");
          return;
        }

        const payload = await res.json();
        const payloadTitle = Object.keys(payload)[0];
        const nextCards = Object.values(payload)[0] || [];

        setResponseTitle(payloadTitle || "");
        setCards(nextCards);
        setMediaState(nextCards.length > 0 ? "results" : "empty");
      } catch (_) {
        setMediaState("error");
      }
    },
    [auth.token, id]
  );

  useEffect(() => {
    setCards([]);
    setResponseTitle("");
    setMediaState("loading");
    dispatch(fetchLibraryScanStatus(id));
    fetchCards();
  }, [dispatch, fetchCards, id]);

  useEffect(() => {
    if (scanState === "complete") fetchCards();
  }, [fetchCards, scanState]);

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
      refreshTimer.current = setTimeout(() => fetchCards(), 500);
    },
    [fetchCards, id]
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

  const handleRetry = async () => {
    setRetrying(true);
    await dispatch(retryLibraryScan(id));
    setRetrying(false);
  };

  return (
    <div className="libraryCards">
      <div className="libraryHeader">
        <h2>{title}</h2>
        <div className="actions">
          <Dropdown />
        </div>
      </div>

      <LibraryState
        mediaState={mediaState}
        mediaType={library?.media_type}
        onRetry={handleRetry}
        retrying={retrying}
        scanState={scanState}
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
