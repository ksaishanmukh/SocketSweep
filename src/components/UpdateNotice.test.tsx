// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { UpdateNotice } from "./UpdateNotice";

const openUrl = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

const release = {
  version: "2.1.0",
  url: "https://github.com/ksaishanmukh/SocketSweep/releases/tag/v2.1.0",
};

it("announces the release, opens it in the system browser and can be dismissed", async () => {
  const onDismiss = vi.fn();
  const user = userEvent.setup();
  render(<UpdateNotice release={release} onDismiss={onDismiss} />);

  expect(screen.getByRole("status").textContent).toContain("SocketSweep 2.1.0 is available");
  await user.click(screen.getByRole("link", { name: "Download" }));
  expect(openUrl).toHaveBeenCalledWith(release.url);
  await user.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(onDismiss).toHaveBeenCalled();
});
