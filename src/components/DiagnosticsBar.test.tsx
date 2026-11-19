// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { commands } from "../api";
import { commandError, resetApi } from "../test/api";
import { DiagnosticsBar } from "./DiagnosticsBar";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  ...(await import("../test/api")).apiMock,
}));

const openPath = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath }));
vi.mock("@tauri-apps/api/path", () => ({ appLogDir: vi.fn().mockResolvedValue("/logs") }));

const api = vi.mocked(commands);

beforeEach(() => {
  resetApi();
  openPath.mockClear();
});
afterEach(cleanup);

/** userEvent installs its own clipboard, so watch that one. */
function setup() {
  const user = userEvent.setup();
  const writeText = vi.spyOn(navigator.clipboard, "writeText").mockClear();
  render(<DiagnosticsBar />);
  return { user, writeText };
}

it("copies the host's report without private details by default", async () => {
  api.diagnostics.mockResolvedValue("REPORT");
  const { user, writeText } = setup();

  await user.click(screen.getByRole("button", { name: "Copy diagnostics" }));

  expect(api.diagnostics).toHaveBeenCalledWith(false);
  expect(writeText).toHaveBeenCalledWith("REPORT");
  expect(screen.getByRole("status").textContent).toContain("Copied");
});

it("asks for the private details only once the box is ticked", async () => {
  api.diagnostics.mockResolvedValue("FULL");
  const { user, writeText } = setup();

  await user.click(screen.getByRole("checkbox", { name: "Include device serial and paths" }));
  await user.click(screen.getByRole("button", { name: "Copy diagnostics" }));

  expect(api.diagnostics).toHaveBeenCalledWith(true);
  expect(writeText).toHaveBeenCalledWith("FULL");
});

it("says so when the report cannot be built, and copies nothing", async () => {
  api.diagnostics.mockRejectedValue(commandError("daemon", "no report"));
  const { user, writeText } = setup();

  await user.click(screen.getByRole("button", { name: "Copy diagnostics" }));

  expect(writeText).not.toHaveBeenCalled();
  expect(screen.getByRole("status").textContent).toContain("no report");
});

it("opens the app's log folder", async () => {
  const { user } = setup();

  await user.click(screen.getByRole("button", { name: "Open log folder" }));

  expect(openPath).toHaveBeenCalledWith("/logs");
});
