import { useState } from "react";
import { useParams } from "react-router";

import { useGetMediaSeasonsQuery } from "../../api/v1/media";

import CardImage from "./CardImage";
import MediaEpisodes from "./Episodes";

import "./Seasons.scss";

function SeasonSelection({ seasons, setActiveId }) {
  const [season, setSeason] = useState(() => seasons[0]?.id);

  return (
    <div className="mediaPageSeasons">
      <section>
        <h2>Seasons</h2>
        <div className={`seasons ${season && "selected"}`}>
          {seasons.map(({ id, season_number, poster }) => (
            <div
              className={`season ${id === season && "active"}`}
              key={id}
              onClick={() => setSeason(id)}
            >
              <CardImage src={poster} />
              <p>Season {season_number}</p>
            </div>
          ))}
        </div>
      </section>
      {season !== undefined && (
        <MediaEpisodes seasonID={season} setActiveId={setActiveId} />
      )}
    </div>
  );
}

function MediaSeasons(props) {
  const { setActiveId } = props;
  const { id } = useParams();
  const { data: seasons } = useGetMediaSeasonsQuery(id);

  if (seasons) {
    return (
      <SeasonSelection key={id} seasons={seasons} setActiveId={setActiveId} />
    );
  }

  return null;
}

export default MediaSeasons;
