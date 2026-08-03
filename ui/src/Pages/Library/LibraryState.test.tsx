import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import LibraryState from "./LibraryState";

const defaultProps = {
  mediaState: "empty" as const,
  mediaType: "movie",
  onRetry: jest.fn(),
  retrying: false,
};

describe("library scan states", () => {
  afterEach(() => jest.clearAllMocks());

  it("shows persistent progress while scanning", () => {
    render(<LibraryState {...defaultProps} scanState="scanning" />);

    expect(screen.getByText("Scanning your library")).toBeInTheDocument();
    expect(
      screen.getByText(/scanning the selected folder/i)
    ).toBeInTheDocument();
  });

  it("shows a clear empty result when a scan completes without media", () => {
    render(<LibraryState {...defaultProps} scanState="complete" />);

    expect(screen.getByText("No supported movies found")).toBeInTheDocument();
    expect(screen.getByText(/finished scanning/i)).toBeInTheDocument();
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

  it("stops progress and offers retry when scanning fails", () => {
    render(<LibraryState {...defaultProps} scanState="failed" />);

    expect(screen.getByRole("alert")).toHaveTextContent("Library scan failed");
    expect(screen.queryByText("Scanning your library")).not.toBeInTheDocument();

    userEvent.click(screen.getByRole("button", { name: "Retry scan" }));
    expect(defaultProps.onRetry).toHaveBeenCalledTimes(1);
  });
});
