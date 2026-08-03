import { useCallback, useEffect, useRef, useState } from "react";
import { useSelector } from "react-redux";
import { useParams } from "react-router";

import Delete from "./Actions/Delete";

import EditIcon from "../../assets/Icons/Edit";

import "./Dropdown.scss";

function Dropdown({ onRescan, scanStarting, scanning }) {
  const dropdownRef = useRef(null);
  const params = useParams();

  const user = useSelector((store) => store.user);

  const [dropdownVisible, setDropdownVisible] = useState(false);

  const handleClick = useCallback((e) => {
    if (!dropdownRef.current) return;

    if (!dropdownRef.current.contains(e.target)) {
      setDropdownVisible(false);
    }
  }, []);

  useEffect(() => {
    window.addEventListener("click", handleClick);

    return () => {
      window.removeEventListener("click", handleClick);
    };
  }, [handleClick]);

  const handleToggle = useCallback(() => {
    setDropdownVisible((visible) => !visible);
  }, []);

  const handleRescan = useCallback(() => {
    setDropdownVisible(false);
    onRescan();
  }, [onRescan]);

  if (!user.info.roles?.includes("owner")) return null;

  return (
    <div className="dropdown" ref={dropdownRef}>
      <button
        type="button"
        className={`toggle visible-${dropdownVisible}`}
        onClick={handleToggle}
        aria-expanded={dropdownVisible}
        aria-haspopup="menu"
      >
        Library actions
      </button>
      <div className={`dropDownContent visible-${dropdownVisible}`} role="menu">
        <button
          type="button"
          className="rescan"
          onClick={handleRescan}
          disabled={scanning || scanStarting}
        >
          {scanning || scanStarting ? "Scanning library…" : "Rescan library"}
        </button>
        <Delete id={params.id} />
        <button className="rename">
          Rename library
          <EditIcon />
        </button>
      </div>
    </div>
  );
}

export default Dropdown;
