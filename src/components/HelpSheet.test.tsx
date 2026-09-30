import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { HelpSheet } from "./HelpSheet";

describe("HelpSheet", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <HelpSheet open={false} onClose={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows overview and switches to speakers", async () => {
    const user = userEvent.setup();
    render(<HelpSheet open onClose={vi.fn()} />);

    expect(screen.getByTestId("help-sheet")).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "Help" })).toBeVisible();
    expect(
      screen.getByRole("heading", { level: 3, name: /What Video to Text does/i }),
    ).toBeVisible();

    await user.click(screen.getByTestId("help-nav-speakers"));
    expect(
      screen.getByRole("heading", { level: 3, name: /Who spoke/i }),
    ).toBeVisible();
    expect(screen.getByText(/Path A — English only/i)).toBeVisible();
  });

  it("closes and opens preferences from the action bar", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const onOpenPreferences = vi.fn();
    render(
      <HelpSheet
        open
        onClose={onClose}
        onOpenPreferences={onOpenPreferences}
      />,
    );

    await user.click(screen.getByTestId("help-open-preferences"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onOpenPreferences).toHaveBeenCalledTimes(1);
  });
});
