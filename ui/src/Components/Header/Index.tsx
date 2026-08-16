import { useCallback, useEffect, useRef, useState } from "react";
import { NavLink, useLocation } from "react-router";

import { fetchLibraries } from "actions/library.js";
import {
  fetchLibraryScanStatus,
  handleWsDelLibrary,
  handleWsNewLibrary,
  wsScanCancelled,
  wsScanFailed,
  wsScanStart,
  wsScanStop,
} from "actions/library.js";
import { fetchGlobalSettings, fetchUserSettings } from "actions/settings.js";
import UserIcon from "assets/Icons/User";
import { useAppDispatch, useAppSelector } from "hooks/store";
import useWebSocket from "hooks/ws";

import NewLibraryModal from "../../Modals/NewLibrary/Index";
import LogoutBtn from "../Sidebar/Profile/LogoutBtn";
import Search from "../Sidebar/Search";

import "./Index.scss";

type Library = {
  id: string | number;
  media_type: "movie" | "tv";
  name: string;
};

type LibraryNavProps = {
  activeId?: string;
  label: string;
  libraries: Library[];
};

function LibraryNav({ activeId, label, libraries }: LibraryNavProps) {
  const isActive = libraries.some(
    (library) => String(library.id) === String(activeId)
  );

  if (libraries.length === 0) {
    return (
      <span className="eclipse-nav-item unavailable" aria-disabled="true">
        {label}
      </span>
    );
  }

  if (libraries.length === 1) {
    return (
      <NavLink
        to={`/library/${libraries[0].id}`}
        className={`eclipse-nav-item${isActive ? " active" : ""}`}
      >
        {label}
      </NavLink>
    );
  }

  return (
    <details className={`eclipse-library-menu${isActive ? " active" : ""}`}>
      <summary className="eclipse-nav-item">{label}</summary>
      <div className="eclipse-menu-content">
        {libraries.map((library) => (
          <NavLink key={library.id} to={`/library/${library.id}`}>
            {library.name}
          </NavLink>
        ))}
      </div>
    </details>
  );
}

function ScanStatus() {
  const { items } = useAppSelector(
    (store) => store.library.fetch_libraries
  ) as { items: Library[] };
  const scanning = useAppSelector((store) => store.library.scanning) as Array<
    string | number
  >;
  const progress = useAppSelector((store) => store.library.scan_progress) as
    | Record<string, { discovered?: number; processed?: number }>
    | undefined;

  if (scanning.length === 0) return null;

  const id = scanning[0];
  const library = items.find((item) => String(item.id) === String(id));
  const scan = progress?.[String(id)];
  const hasProgress = Boolean(scan?.discovered && scan.discovered > 0);
  const remaining = scanning.length - 1;

  return (
    <div className="eclipse-scan-status" role="status" aria-live="polite">
      <span>
        Scanning {library?.name || "library"}
        {hasProgress ? ` ${scan?.processed || 0}/${scan?.discovered}` : ""}
        {remaining > 0 ? ` +${remaining}` : ""}
      </span>
      <span className="eclipse-spinner" aria-hidden="true" />
    </div>
  );
}

function ProfileMenu() {
  const user = useAppSelector((store) => store.user);
  const menuRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (
        menuRef.current &&
        event.target instanceof Node &&
        !menuRef.current.contains(event.target)
      ) {
        setOpen(false);
      }
    };

    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, []);

  return (
    <div className="eclipse-profile" ref={menuRef}>
      <button
        className="eclipse-profile-button"
        type="button"
        aria-label="Open profile menu"
        aria-expanded={open}
        onClick={() => setOpen((visible) => !visible)}
      >
        {user.info.picture ? (
          <img src={user.info.picture} alt="" />
        ) : (
          <UserIcon />
        )}
      </button>
      {open && (
        <div className="eclipse-profile-menu">
          {user.info.username && (
            <p className="eclipse-profile-name">{user.info.username}</p>
          )}
          <NavLink to="/" end onClick={() => setOpen(false)}>
            Dashboard
          </NavLink>
          <NavLink to="/preferences" onClick={() => setOpen(false)}>
            Preferences
          </NavLink>
          <LogoutBtn />
        </div>
      )}
    </div>
  );
}

function Header() {
  const dispatch = useAppDispatch();
  const ws = useWebSocket();
  const location = useLocation();
  const user = useAppSelector((store) => store.user);
  const libraries = useAppSelector(
    (store) => store.library.fetch_libraries
  ) as { fetched: boolean; items: Library[] };
  const scanning = useAppSelector((store) => store.library.scanning) as Array<
    string | number
  >;

  useEffect(() => {
    dispatch(fetchLibraries());
    dispatch(fetchUserSettings());
    dispatch(fetchGlobalSettings());
  }, [dispatch]);

  const handleWS = useCallback(
    ({ data }: MessageEvent) => {
      const payload = JSON.parse(data);
      const handlers: Record<string, (id: string | number) => unknown> = {
        EventStartedScanning: wsScanStart,
        EventStoppedScanning: wsScanStop,
        EventScanFailed: wsScanFailed,
        EventScanCancelled: wsScanCancelled,
        EventNewLibrary: handleWsNewLibrary,
        EventRemoveLibrary: handleWsDelLibrary,
      };
      const handler = handlers[payload.type];
      if (handler) dispatch(handler(payload.id) as never);
    },
    [dispatch]
  );

  useEffect(() => {
    if (!ws) return;
    ws.addEventListener("message", handleWS);
    return () => ws.removeEventListener("message", handleWS);
  }, [handleWS, ws]);

  useEffect(() => {
    if (!libraries.fetched) return;
    libraries.items.forEach((library) => {
      dispatch(fetchLibraryScanStatus(library.id));
    });
  }, [dispatch, libraries.fetched, libraries.items]);

  useEffect(() => {
    if (scanning.length === 0) return;
    const timer = window.setInterval(() => {
      scanning.forEach((libraryId) => {
        dispatch(fetchLibraryScanStatus(libraryId));
      });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [dispatch, scanning]);

  const movieLibraries = libraries.items.filter(
    (library) => library.media_type === "movie"
  );
  const showLibraries = libraries.items.filter(
    (library) => library.media_type === "tv"
  );
  const activeLibraryId = location.pathname.match(/^\/library\/([^/]+)/)?.[1];
  const canAddLibrary = user.info.roles?.includes("owner");

  return (
    <header className="eclipse-header">
      <Search variant="header" />
      <nav className="eclipse-library-nav" aria-label="Libraries">
        <LibraryNav
          label="Movies"
          libraries={movieLibraries}
          activeId={activeLibraryId}
        />
        <LibraryNav
          label="Shows"
          libraries={showLibraries}
          activeId={activeLibraryId}
        />
        <span
          className="eclipse-nav-item unavailable"
          aria-disabled="true"
          title="Watchlist is reserved for a future library"
        >
          Watchlist
        </span>
        {canAddLibrary ? (
          <NewLibraryModal>
            <button className="eclipse-nav-item eclipse-add" type="button">
              Add
            </button>
          </NewLibraryModal>
        ) : (
          <span className="eclipse-nav-item unavailable" aria-disabled="true">
            Add
          </span>
        )}
      </nav>
      <div className="eclipse-header-end">
        <ScanStatus />
        <ProfileMenu />
      </div>
    </header>
  );
}

export default Header;
