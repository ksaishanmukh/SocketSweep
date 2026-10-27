// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { commands } from "../../api";
import { commandError, resetApi } from "../../test/api";
import { SetupScreen } from "./SetupScreen";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  ...(await import("../../test/api")).apiMock,
}));

const api = vi.mocked(commands);

beforeEach(() => resetApi());
afterEach(cleanup);

describe("SetupScreen", () => {
  it("says why when the host cannot list devices, e.g. adb will not start", async () => {
    api.listDevices.mockRejectedValue(commandError("device", "Could not start adb."));

    render(<SetupScreen onConnect={() => {}} loading={false} />);

    expect((await screen.findByRole("alert")).textContent).toBe("Could not start adb.");
    expect(screen.queryByText(/USB Debugging enabled/)).toBeNull();
  });
});
