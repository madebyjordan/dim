import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Mock } from "vitest";

import { useGetDirectoriesQuery } from "../../api/v1/fileBrowser";
import DirSelection from "./DirSelection";

vi.mock("../../api/v1/fileBrowser", () => ({
  useGetDirectoriesQuery: vi.fn(),
}));

const mockedUseGetDirectoriesQuery = useGetDirectoriesQuery as Mock;

function Harness() {
  const [current, setCurrent] = useState<string | undefined>();
  const [selectedFolder, setSelectedFolder] = useState<string | undefined>();

  return (
    <DirSelection
      current={current}
      setCurrent={setCurrent}
      selectedFolder={selectedFolder}
      setSelectedFolder={setSelectedFolder}
    />
  );
}

describe("library folder selection", () => {
  beforeEach(() => {
    mockedUseGetDirectoriesQuery.mockImplementation((path?: string) => {
      const current = path ?? "/Users/test";
      return {
        data: {
          current,
          parent:
            current === "/"
              ? null
              : current.split("/").slice(0, -1).join("/") || "/",
          directories:
            current === "/Users/test"
              ? [{ name: "Movies", path: "/Users/test/Movies" }]
              : [],
        },
        error: undefined,
        isFetching: false,
      };
    });
  });

  afterEach(() => vi.clearAllMocks());

  it("browses into a child and uses the current folder", async () => {
    render(<Harness />);

    const movies = await screen.findByRole("button", { name: "Movies" });
    userEvent.click(movies);

    await waitFor(() =>
      expect(mockedUseGetDirectoriesQuery).toHaveBeenCalledWith(
        "/Users/test/Movies"
      )
    );

    userEvent.click(screen.getByRole("button", { name: "Use this folder" }));
    expect(
      screen.getByRole("button", { name: "Folder selected" })
    ).toBeInTheDocument();
  });

  it("does not expose multi-select or manual-path controls", async () => {
    render(<Harness />);

    await screen.findByRole("button", { name: "Movies" });
    expect(
      screen.queryByPlaceholderText("Enter an absolute folder path")
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Select visible")).not.toBeInTheDocument();
    expect(screen.queryByText("Currently in:")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("never renders an HTML fallback response as a folder error", async () => {
    mockedUseGetDirectoriesQuery.mockReturnValue({
      data: undefined,
      error: {
        status: "PARSING_ERROR",
        data: "<!doctype html><html><body>Dim application</body></html>",
      },
      isFetching: false,
    });

    render(<Harness />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The folder browser is unavailable. Restart Dim and try again."
    );
    expect(screen.queryByText(/doctype html/i)).not.toBeInTheDocument();
  });
});
