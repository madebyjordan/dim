import { useCallback, useEffect, useState } from "react";

import { useGetDirectoriesQuery } from "../../api/v1/fileBrowser";
import FolderIcon from "../../assets/Icons/Folder";
import Button from "../../Components/Misc/Button";
import ChevronRight from "../../assets/Icons/ChevronRight";
import ChevronLeft from "../../assets/Icons/ChevronLeft";

import "./DirSelection.scss";

interface Props {
  current?: string;
  setCurrent: React.Dispatch<React.SetStateAction<string | undefined>>;
  selectedFolder?: string;
  setSelectedFolder: React.Dispatch<React.SetStateAction<string | undefined>>;
}

const EMPTY_DIRECTORIES: Array<{ name: string; path: string }> = [];

function browserErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "data" in error) {
    const data = (error as { data?: unknown }).data;
    if (
      typeof data === "string" &&
      data.length > 0 &&
      data.length <= 200 &&
      !data.includes("<")
    ) {
      return data;
    }
  }

  return "The folder browser is unavailable. Restart Dim and try again.";
}

function DirSelection(props: Props) {
  const { current, setCurrent, selectedFolder, setSelectedFolder } = props;
  const [forwardHistory, setForwardHistory] = useState<string[]>([]);

  const { data, error, isFetching } = useGetDirectoriesQuery(current);
  const directories = data?.directories ?? EMPTY_DIRECTORIES;
  const listedCurrent = data?.current;

  useEffect(() => {
    if (!listedCurrent) return;
    setCurrent((currentPath) => currentPath ?? listedCurrent);
  }, [listedCurrent, setCurrent]);

  const navigateTo = useCallback(
    (path: string) => {
      setForwardHistory([]);
      setCurrent(path);
    },
    [setCurrent]
  );

  const goBack = useCallback(() => {
    if (!data?.parent) return;

    setForwardHistory((history) => [...history, data.current]);
    setCurrent(data.parent);
  }, [data, setCurrent]);

  const goForward = useCallback(() => {
    setForwardHistory((history) => {
      if (history.length === 0) return history;

      const next = [...history];
      setCurrent(next.pop());
      return next;
    });
  }, [setCurrent]);

  let directoryContent;
  if (isFetching && !data) {
    directoryContent = (
      <div className="vertical-err" role="status">
        <p>Loading folders…</p>
      </div>
    );
  } else if (error) {
    directoryContent = (
      <div className="vertical-err" role="alert">
        <p>{browserErrorMessage(error)}</p>
      </div>
    );
  } else if (directories.length === 0) {
    directoryContent = (
      <div className="vertical-err">
        <p>No folders inside this location.</p>
      </div>
    );
  } else {
    directoryContent = directories.map((directory) => (
      <button
        type="button"
        key={directory.path}
        className="dir"
        onClick={() => navigateTo(directory.path)}
      >
        <FolderIcon />
        <span>{directory.name}</span>
        <ChevronRight />
      </button>
    ));
  }

  const currentFolderSelected = Boolean(
    data && selectedFolder === data.current
  );

  return (
    <div className="dirSelection">
      <div className="controls">
        <Button
          onClick={goBack}
          disabled={!data?.parent}
          type="secondary contrast"
        >
          <ChevronLeft />
        </Button>
        <Button
          onClick={goForward}
          disabled={forwardHistory.length === 0}
          type="secondary contrast"
        >
          <ChevronRight />
        </Button>
      </div>
      <div className="dirs-wrapper">
        <div className="dirs" aria-busy={isFetching}>
          {directoryContent}
        </div>
      </div>
      <div className="folder-action">
        <Button
          onClick={() => data && setSelectedFolder(data.current)}
          disabled={!data || Boolean(error) || currentFolderSelected}
        >
          {currentFolderSelected ? "Folder selected" : "Use this folder"}
        </Button>
      </div>
    </div>
  );
}

export default DirSelection;
