import { useEffect, useRef, useState } from "react";
import { shallowEqual, useSelector } from "react-redux";
import { Link } from "react-router";

import HoverCard from "./HoverCard";
import Image from "./Image";

import "./Index.scss";

function Card(props) {
  const { settings } = useSelector(
    (store) => ({
      settings: store.settings.userSettings,
    }),
    shallowEqual
  );

  const cardWrapper = useRef(null);
  const hoverCard = useRef(null);
  const card = useRef(null);

  const [hovering, setHovering] = useState(false);
  const [hoverCardSide, setHoverCardSide] = useState("right");
  const timeoutID = useRef(null);

  useEffect(() => {
    return () => clearTimeout(timeoutID.current);
  }, []);

  const showPopup = () => {
    setHovering(true);
  };

  const onMouseLeave = () => {
    clearTimeout(timeoutID.current);

    hoverCard.current?.classList.add("hideCardPopup");
  };

  const handleMouseEnter = () => {
    // removes cardHighlight animation (when searched for)
    if (card.current && card.current.style.animation) {
      card.current.style.animation = "";
    }

    if (hovering || window.innerWidth < 1400 || !settings.data.show_hovercards)
      return;

    const rect = card.current.getBoundingClientRect();

    const hoverCardWidth = parseInt(
      getComputedStyle(document.documentElement).getPropertyValue(
        "--hoverCardWidth"
      )
    );

    const showHoverOnRight = window.innerWidth - rect.right > hoverCardWidth;
    const side = showHoverOnRight ? "right" : "left";

    setHoverCardSide(side);

    timeoutID.current = setTimeout(showPopup, 600);
  };

  const { name, poster_path, id, media_type } = props.data;
  const mediaProgress =
    media_type === "movie"
      ? (props.data.progress / props.data.duration) * 100
      : 0;

  return (
    <div
      className="card-wrapper"
      ref={cardWrapper}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <div id={id} className="card" ref={card}>
        <Link to={`/media/${id}`}>
          <Image src={poster_path} progress={mediaProgress} />
          {settings.data.show_card_names && (
            <p style={{ opacity: +!hovering }}>{name}</p>
          )}
        </Link>
      </div>
      {hovering && (
        <HoverCard
          side={hoverCardSide}
          popup={hoverCard}
          data={props.data}
          setHovering={setHovering}
        />
      )}
    </div>
  );
}

export default Card;
