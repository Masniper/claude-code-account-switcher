import { afterEach, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { Meter } from "./components";
import { CopyProvider } from "./i18n";

afterEach(cleanup);

const meter = (percent: number) => {
  render(
    <CopyProvider lang="en">
      <Meter label="5-hour window" value={{ percent, resets_at: null }} now={0} />
    </CopyProvider>,
  );
  return screen.getByRole("meter");
};

it("tells a screen reader the same figure the label shows, even above 100", () => {
  // Regression: `aria-valuenow` was clamped to 100 while `aria-valuetext` and
  // the visible label said 120, so one element reported two numbers.
  const over = meter(120);
  expect(screen.getByText("120%")).toBeTruthy();
  expect(over.getAttribute("aria-valuetext")).toBe("120%");
  expect(over.getAttribute("aria-valuenow")).toBe("120");
  expect(over.getAttribute("aria-valuemax")).toBe("120");
  cleanup();

  const normal = meter(42);
  expect(normal.getAttribute("aria-valuenow")).toBe("42");
  expect(normal.getAttribute("aria-valuemax")).toBe("100");
  expect(normal.getAttribute("aria-valuetext")).toBe("42%");
});
