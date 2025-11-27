import { createConnection } from "node:net";
import { FsHasherError } from "./types.js";

export const SOCKET_PATH =
  process.platform === "win32"
    ? "\\\\.\\pipe\\fs-hasher"
    : "/tmp/fs-hasher.sock";

export function connect() {
  return createConnection(SOCKET_PATH);
}

export async function request<T>(data: Record<string, unknown>): Promise<T> {
  return new Promise((resolve, reject) => {
    const socket = connect();
    let response = "";

    socket.on("connect", () => {
      socket.write(JSON.stringify(data) + "\n");
    });

    socket.on("data", (chunk) => {
      response += chunk.toString();
      if (response.includes("\n")) {
        socket.end();
      }
    });

    socket.on("end", () => {
      try {
        const line = response.split("\n")[0];
        if (!line) {
          reject(new FsHasherError("Empty response from daemon"));
          return;
        }
        resolve(JSON.parse(line) as T);
      } catch {
        reject(new FsHasherError(`Invalid response: ${response}`));
      }
    });

    socket.on("error", (err) => {
      reject(new FsHasherError(`Connection failed: ${err.message}`, "ECONNREFUSED"));
    });
  });
}
