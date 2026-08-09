import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import SimpleSearch from "./SimpleSearch";

vi.mock("assets/figma_icons/Search", () => ({ default: () => null }));

it("reports the current query directly from the input event", () => {
  const onChange = vi.fn();

  render(<SimpleSearch onChange={onChange} />);
  expect(onChange).not.toHaveBeenCalled();

  userEvent.type(screen.getByRole("textbox"), "Alien");

  expect(onChange).toHaveBeenLastCalledWith("Alien");
});
