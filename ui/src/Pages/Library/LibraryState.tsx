import BarLoad from "../../Components/Load/Bar";

export type LibraryMediaState = "loading" | "empty" | "results" | "error";
export type LibraryScanState = "scanning" | "complete" | "failed";

interface LibraryStateProps {
  mediaState: LibraryMediaState;
  mediaType?: string;
  onRetry: () => void;
  retrying: boolean;
  scanState?: LibraryScanState;
}

const LibraryState = ({
  mediaState,
  mediaType,
  onRetry,
  retrying,
  scanState,
}: LibraryStateProps) => {
  if (scanState === "scanning") {
    return (
      <section className="libraryState scanning" aria-live="polite">
        <h3>Scanning your library</h3>
        <p>
          Dim is scanning the selected folder for supported{" "}
          {mediaType === "tv" ? "shows" : "movies"}.
        </p>
        <BarLoad />
      </section>
    );
  }

  if (scanState === "failed") {
    return (
      <section className="libraryState failed" role="alert">
        <h3>Library scan failed</h3>
        <p>
          Dim could not finish scanning this folder. Check that the folder is
          available and readable, then try again.
        </p>
        <button type="button" onClick={onRetry} disabled={retrying}>
          {retrying ? "Starting scan…" : "Retry scan"}
        </button>
      </section>
    );
  }

  if (mediaState === "results") return null;

  if (!scanState || mediaState === "loading") {
    return (
      <section className="libraryState loading" aria-live="polite">
        <h3>Loading library…</h3>
      </section>
    );
  }

  if (mediaState === "error") {
    return (
      <section className="libraryState failed" role="alert">
        <h3>Library unavailable</h3>
        <p>Dim could not load this library. Refresh the page to try again.</p>
      </section>
    );
  }

  if (mediaState === "empty") {
    return (
      <section className="libraryState empty" aria-live="polite">
        <h3>No supported {mediaType === "tv" ? "shows" : "movies"} found</h3>
        <p>
          Dim finished scanning the selected folder, but did not find any media
          it can add to this library.
        </p>
      </section>
    );
  }

  return null;
};

export default LibraryState;
