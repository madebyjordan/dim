import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import Toggle from "./Toggle";

it("uses its state prop as the source of truth", () => {
  const onToggle = vi.fn();
  const { rerender } = render(
    <Toggle name="Setting" state={false} onToggle={onToggle} />
  );

  const control = screen.getByText("Setting").parentElement?.lastElementChild;
  expect(control).toHaveClass("active-false");

  userEvent.click(control as Element);
  expect(onToggle).toHaveBeenCalledWith(true);
  expect(control).toHaveClass("active-false");

  rerender(<Toggle name="Setting" state onToggle={onToggle} />);
  expect(control).toHaveClass("active-true");
});
