import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import LibraryState from "./LibraryState";

const defaultProps = {
  canRescan: true,
  mediaState: "empty" as const,
  mediaType: "movie",
  onRescan: vi.fn(),
  scanStarting: false,
};

describe("library scan states", () => {
  afterEach(() => vi.clearAllMocks());

  it("shows persistent progress while scanning", () => {
    render(
      <LibraryState
        {...defaultProps}
        scanState="scanning"
        scanProgress={{
          stage: "matching",
          discovered: 20,
          processed: 12,
          committed: 8,
          skipped: 3,
          failed: 1,
          elapsed_seconds: 65,
          seconds_since_progress: 4,
        }}
      />
    );

    expect(screen.getByText("Scanning your library")).toBeInTheDocument();
    expect(screen.getByText("Matching metadata")).toBeInTheDocument();
    expect(
      screen.getByText(/20 discovered · 12 processed/)
    ).toBeInTheDocument();
    expect(screen.getByText(/Elapsed 1m 5s/)).toBeInTheDocument();
  });

  it("shows a clear empty result when a scan completes without media", () => {
    render(<LibraryState {...defaultProps} scanState="complete" />);

    expect(screen.getByText("No supported movies found")).toBeInTheDocument();
    expect(screen.getByText(/finished scanning/i)).toBeInTheDocument();
    userEvent.click(screen.getByRole("button", { name: "Scan library" }));
    expect(defaultProps.onRescan).toHaveBeenCalledTimes(1);
  });

  it("leaves the normal library view unobstructed when results exist", () => {
    const { container } = render(
      <LibraryState
        {...defaultProps}
        mediaState="results"
        scanState="complete"
      />
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("does not offer scan controls to non-owner users", () => {
    render(
      <LibraryState {...defaultProps} canRescan={false} scanState="complete" />
    );

    expect(
      screen.queryByRole("button", { name: "Scan library" })
    ).not.toBeInTheDocument();
  });

  it("stops progress and offers retry when scanning fails", () => {
    render(
      <LibraryState
        {...defaultProps}
        scanState="failed"
        scanProgress={{
          error_summary: "Network share is unavailable; retry the scan",
        }}
      />
    );

    expect(screen.getByRole("alert")).toHaveTextContent("Library scan failed");
    expect(screen.queryByText("Scanning your library")).not.toBeInTheDocument();
    expect(
      screen.getByText(/Network share is unavailable/)
    ).toBeInTheDocument();

    userEvent.click(screen.getByRole("button", { name: "Retry scan" }));
    expect(defaultProps.onRescan).toHaveBeenCalledTimes(1);
  });

  it("stops progress and offers retry when scanning is cancelled", () => {
    render(<LibraryState {...defaultProps} scanState="cancelled" />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Library scan cancelled"
    );
    expect(screen.queryByText("Scanning your library")).not.toBeInTheDocument();
    userEvent.click(screen.getByRole("button", { name: "Retry scan" }));
    expect(defaultProps.onRescan).toHaveBeenCalledTimes(1);
  });
});
