import { execSync } from "node:child_process";

const BINARY_NAME = process.platform === "win32" ? "fswatchd.exe" : "fswatchd";

export class FswatchdNotFoundError extends Error {
  constructor() {
    super(
      `fswatchd binary not found in PATH. Install it:\n` +
      `  curl -fsSL https://raw.githubusercontent.com/altinokdarici/fs-hasher/main/install.sh | sh\n` +
      `\n` +
      `Or install via cargo:\n` +
      `  cargo install fswatchd`
    );
    this.name = "FswatchdNotFoundError";
  }
}

export function getBinaryPath(): string {
  const command = process.platform === "win32" ? "where" : "which";

  try {
    const result = execSync(`${command} ${BINARY_NAME}`, { encoding: "utf8" });
    const path = result.trim().split("\n")[0];
    if (path) {
      return path;
    }
  } catch {
    // Binary not found in PATH
  }

  throw new FswatchdNotFoundError();
}
