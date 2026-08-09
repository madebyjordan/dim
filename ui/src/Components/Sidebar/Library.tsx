import { NavLink } from "react-router";

import { useAppSelector } from "hooks/store";
import FilmIcon from "assets/Icons/Film";
import TvIcon from "assets/Icons/TvIcon";
import BarLoad from "Components/Load/Bar";

interface Props {
  id: string;
  media_type: string;
  name: string;
}

function Library(props: Props) {
  const scanning = useAppSelector((store) => store.library.scanning);
  const { id, media_type, name } = props;
  const isScanning = scanning.some((scanId) => String(scanId) === String(id));

  return (
    <NavLink
      to={"/library/" + id}
      className={({ isActive }) =>
        `item showLoad-${isScanning}${isActive ? " active" : ""}`
      }
    >
      {media_type === "movie" && <FilmIcon />}
      {media_type === "tv" && <TvIcon />}
      <p>{name}</p>
      {isScanning && <BarLoad />}
    </NavLink>
  );
}

export default Library;
