import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import Dropdown from "./Dropdown";

vi.mock("react-redux", () => ({
  useSelector: (selector) => selector({ user: { info: { roles: ["owner"] } } }),
}));

vi.mock("react-router", () => ({
  useParams: () => ({ id: "42" }),
}));

vi.mock("./Actions/Delete", () => ({
  default: () => <button>Delete library</button>,
}));
vi.mock("../../assets/Icons/Edit", () => ({ default: () => null }));

describe("library actions", () => {
  it("offers a manual rescan for an existing library", () => {
    const onRescan = vi.fn();
    render(
      <Dropdown onRescan={onRescan} scanStarting={false} scanning={false} />
    );

    userEvent.click(screen.getByRole("button", { name: "Library actions" }));
    userEvent.click(screen.getByRole("button", { name: "Rescan library" }));

    expect(onRescan).toHaveBeenCalledTimes(1);
  });

  it("disables the rescan action while a scan is active", () => {
    render(
      <Dropdown onRescan={vi.fn()} scanStarting={false} scanning={true} />
    );

    userEvent.click(screen.getByRole("button", { name: "Library actions" }));
    expect(
      screen.getByRole("button", { name: "Scanning library…" })
    ).toBeDisabled();
  });
});
