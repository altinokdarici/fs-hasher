import { spawn } from "node:child_process";
import { createConnection } from "node:net";
import { SOCKET_PATH } from "./connection.js";
import { getBinaryPath } from "./binary.js";

const MAX_RETRIES = 10;
const RETRY_DELAY_MS = 200;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function isRunning(): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = createConnection(SOCKET_PATH);
    socket.on("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.on("error", () => resolve(false));
  });
}

export async function ensureDaemon(): Promise<void> {
  if (await isRunning()) return;

  const binary = getBinaryPath();
  spawn(binary, ["daemon", "start"], {
    detached: true,
    stdio: "ignore",
  }).unref();

  for (let i = 0; i < MAX_RETRIES; i++) {
    await sleep(RETRY_DELAY_MS);
    if (await isRunning()) return;
  }

  throw new Error("Failed to start fs-hasher daemon");
}
