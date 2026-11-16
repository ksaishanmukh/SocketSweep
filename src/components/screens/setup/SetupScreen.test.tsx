// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { commands, type Device } from "../../../api";
import { commandError, resetApi } from "../../../test/api";
import { SetupScreen } from "./SetupScreen";

vi.mock("../../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../api")>()),
  ...(await import("../../../test/api")).apiMock,
}));

const api = vi.mocked(commands);

const phone = (over: Partial<Device> = {}): Device => ({
  serial: "R5CT1234",
  model: "Pixel 8",
  state: "ready",
  usable: true,
  hint: null,
  ...over,
});

beforeEach(() => resetApi());
afterEach(cleanup);

describe("SetupScreen", () => {
  it("says why when the host cannot list devices, e.g. adb will not start", async () => {
    api.listDevices.mockRejectedValue(commandError("device", "Could not start adb."));

    render(<SetupScreen onConnect={() => {}} loading={false} />);

    expect((await screen.findByRole("alert")).textContent).toBe("Could not start adb.");
  });

  it("offers to scan the only ready phone", async () => {
    api.listDevices.mockResolvedValue([phone()]);
    const onConnect = vi.fn();

    render(<SetupScreen onConnect={onConnect} loading={false} />);
    fireEvent.click(await screen.findByRole("button", { name: "Scan this phone" }));

    expect(screen.getByText("Pixel 8")).toBeTruthy();
    expect(onConnect).toHaveBeenCalledWith();
  });

  it("makes the choice explicit with several phones", async () => {
    api.listDevices.mockResolvedValue([phone(), phone({ serial: "ZZ99", model: "Moto" })]);
    const onConnect = vi.fn();

    render(<SetupScreen onConnect={onConnect} loading={false} />);
    fireEvent.click(await screen.findByRole("button", { name: /Moto/ }));

    expect(onConnect).toHaveBeenCalledWith("ZZ99");
  });

  it("walks through plugging in when no phone is attached", async () => {
    render(<SetupScreen onConnect={() => {}} loading={false} />);

    expect(await screen.findByText("Connect your phone")).toBeTruthy();
    expect(screen.getByText("Turn on USB debugging.")).toBeTruthy();
    expect(screen.getByText("Looking for a phone…")).toBeTruthy();
  });

  it("asks the person to allow USB debugging while unauthorized", async () => {
    api.listDevices.mockResolvedValue([phone({ state: "unauthorized", usable: false })]);

    render(<SetupScreen onConnect={() => {}} loading={false} />);

    expect(await screen.findByText("Waiting for permission")).toBeTruthy();
    const scan = screen.getByRole("button", { name: "Scan this phone" }) as HTMLButtonElement;
    expect(scan.disabled).toBe(true);
  });

  it("shows the checklist, and announces the status, while connecting", () => {
    render(<SetupScreen onConnect={() => {}} loading status="Starting scan…" />);

    expect(screen.getByText("Phone connected")).toBeTruthy();
    expect(screen.getByText("Starting the scanner on the phone")).toBeTruthy();
    expect(screen.getByText("Reading storage")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("Starting scan…");
  });
});
