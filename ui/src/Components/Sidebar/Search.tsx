import {
  useEffect,
  useRef,
  useState,
  useCallback,
  type ChangeEvent,
  type KeyboardEvent,
} from "react";
import { useNavigate } from "react-router";

import QuickSearchResults from "./QuickSearchResults";
import SearchIcon from "../../assets/Icons/Search";

import "./Search.scss";

interface Props {
  variant?: "sidebar" | "header";
}

function Search({ variant = "sidebar" }: Props) {
  const navigate = useNavigate();

  const searchBox = useRef<HTMLDivElement>(null);
  const inputBox = useRef<HTMLInputElement>(null);

  const [query, setQuery] = useState<string | null>(null);
  const [showResults, setShowResults] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const handleClick = useCallback(
    (e: MouseEvent) => {
      if (searchBox.current) {
        if (e.target instanceof Node && searchBox.current.contains(e.target)) {
          setShowResults(true);
        } else {
          setShowResults(false);
          if (!query) setExpanded(false);
        }
      }
    },
    [query]
  );

  useEffect(() => {
    window.addEventListener("click", handleClick);

    return () => {
      window.removeEventListener("click", handleClick);
    };
  }, [handleClick]);

  const handleOnChange = useCallback((e: ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setShowResults(e.target.value.length > 1);
  }, []);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (query && query.length > 1 && e.keyCode === 13) {
        navigate({
          pathname: "/search",
          search: `?query=${encodeURIComponent(query || "")}`,
        });

        setQuery("");
        setShowResults(false);
        if (variant === "header") setExpanded(false);
      }
    },
    [navigate, query, variant]
  );

  const fullSearch = useCallback(() => {
    if (!query && variant === "header") {
      setExpanded(true);
      inputBox.current?.focus();
      return;
    }

    if (query && query.length >= 1) {
      navigate({
        pathname: "/search",
        search: `?query=${encodeURIComponent(query)}`,
      });

      setQuery("");
      setShowResults(false);
      if (variant === "header") setExpanded(false);
    }
  }, [navigate, query, variant]);

  return (
    <div className={`search-box${expanded ? " expanded" : ""}`} ref={searchBox}>
      <div className="search-box-wrapper">
        <input
          ref={inputBox}
          value={query || ""}
          onKeyDown={onKeyDown}
          onChange={handleOnChange}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck="false"
          placeholder="Search"
          type="search"
        />
        <button type="button" aria-label="Search" onClick={fullSearch}>
          <SearchIcon />
        </button>
      </div>
      {query && showResults && <QuickSearchResults query={query} />}
    </div>
  );
}

export default Search;
