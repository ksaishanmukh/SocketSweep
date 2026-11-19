import { commands } from "../api";

/** Sends a console line to the log file. Losing one must never disturb the GUI. */
export async function writeLogLine(line: string): Promise<void> {
  try {
    await commands.logUi(line);
  } catch {
    // Outside the Tauri shell there is no file to write.
  }
}
