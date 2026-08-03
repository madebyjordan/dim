import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import Dropdown from "./Dropdown";

jest.mock("react-redux", () => ({
  useSelector: (selector) => selector({ user: { info: { roles: ["owner"] } } }),
}));

jest.mock("react-router", () => ({
  useParams: () => ({ id: "42" }),
}));

jest.mock("./Actions/Delete", () => () => <button>Delete library</button>);
jest.mock("../../assets/Icons/Edit", () => () => null);

describe("library actions", () => {
  it("offers a manual rescan for an existing library", () => {
    const onRescan = jest.fn();
    render(
      <Dropdown onRescan={onRescan} scanStarting={false} scanning={false} />
    );

    userEvent.click(screen.getByRole("button", { name: "Library actions" }));
    userEvent.click(screen.getByRole("button", { name: "Rescan library" }));

    expect(onRescan).toHaveBeenCalledTimes(1);
  });

  it("disables the rescan action while a scan is active", () => {
    render(
      <Dropdown onRescan={jest.fn()} scanStarting={false} scanning={true} />
    );

    userEvent.click(screen.getByRole("button", { name: "Library actions" }));
    expect(
      screen.getByRole("button", { name: "Scanning library…" })
    ).toBeDisabled();
  });
});
