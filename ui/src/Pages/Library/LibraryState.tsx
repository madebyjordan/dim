import BarLoad from "../../Components/Load/Bar";

export type LibraryMediaState = "loading" | "empty" | "results" | "error";
export type LibraryScanState = "scanning" | "complete" | "failed" | "cancelled";

export interface LibraryScanProgress {
  stage?: string;
  discovered?: number;
  processed?: number;
  committed?: number;
  skipped?: number;
  failed?: number;
  elapsed_seconds?: number;
  seconds_since_progress?: number | null;
  error_summary?: string | null;
}

interface LibraryStateProps {
  canRescan: boolean;
  mediaState: LibraryMediaState;
  mediaType?: string;
  onRescan: () => void;
  scanStarting: boolean;
  scanState?: LibraryScanState;
  scanProgress?: LibraryScanProgress;
}

const stageLabels: Record<string, string> = {
  queued: "Waiting for a scanner worker",
  starting: "Starting scan",
  traversal: "Discovering and probing files",
  matching: "Matching metadata",
  reconciliation: "Reconciling the library",
  complete: "Scan complete",
  failed: "Scan failed",
  cancelled: "Scan cancelled",
};

const duration = (seconds = 0) => {
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return seconds < 3600
    ? `${minutes}m ${seconds % 60}s`
    : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
};

const LibraryState = ({
  canRescan,
  mediaState,
  mediaType,
  onRescan,
  scanStarting,
  scanState,
  scanProgress,
}: LibraryStateProps) => {
  if (scanState === "scanning") {
    return (
      <section className="libraryState scanning" aria-live="polite">
        <h3>Scanning your library</h3>
        <p>
          {stageLabels[scanProgress?.stage || ""] ||
            "Scanning the selected folder"}
        </p>
        <p>
          {scanProgress?.discovered ?? 0} discovered ·{" "}
          {scanProgress?.processed ?? 0} processed ·{" "}
          {scanProgress?.committed ?? 0} added · {scanProgress?.skipped ?? 0}{" "}
          skipped · {scanProgress?.failed ?? 0} failed
        </p>
        <p>
          Elapsed {duration(scanProgress?.elapsed_seconds)} · Last durable
          progress {duration(scanProgress?.seconds_since_progress ?? 0)} ago
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
          {scanProgress?.error_summary ||
            "Dim could not finish scanning this folder. Check that the folder is available and readable, then try again."}
        </p>
        <p>
          {scanProgress?.processed ?? 0} processed ·{" "}
          {scanProgress?.committed ?? 0} added · {scanProgress?.skipped ?? 0}{" "}
          skipped · {scanProgress?.failed ?? 0} failed
        </p>
        {canRescan && (
          <button type="button" onClick={onRescan} disabled={scanStarting}>
            {scanStarting ? "Starting scan…" : "Retry scan"}
          </button>
        )}
      </section>
    );
  }

  if (scanState === "cancelled") {
    return (
      <section className="libraryState failed" role="alert">
        <h3>Library scan cancelled</h3>
        <p>
          {scanProgress?.error_summary ||
            "The scan stopped before it could safely update this library."}
        </p>
        {canRescan && (
          <button type="button" onClick={onRescan} disabled={scanStarting}>
            {scanStarting ? "Starting scan…" : "Retry scan"}
          </button>
        )}
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
        {canRescan && (
          <button type="button" onClick={onRescan} disabled={scanStarting}>
            {scanStarting ? "Starting scan…" : "Scan library"}
          </button>
        )}
      </section>
    );
  }

  return null;
};

export default LibraryState;
