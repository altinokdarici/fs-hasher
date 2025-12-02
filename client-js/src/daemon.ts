import { spawn } from "node:child_process";
import { isConnectable } from "./connection.js";
import { getBinaryPath } from "./binary.js";

const MAX_RETRIES = 10;
const RETRY_DELAY_MS = 200;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function ensureDaemon(): Promise<void> {
  if (await isConnectable()) return;

  const binary = getBinaryPath();
  spawn(binary, ["start"], {
    detached: true,
    stdio: "ignore",
  }).unref();

  for (let i = 0; i < MAX_RETRIES; i++) {
    await sleep(RETRY_DELAY_MS);
    if (await isConnectable()) return;
  }

  throw new Error("Failed to start fswatchd daemon");
}
