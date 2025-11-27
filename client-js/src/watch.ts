import { createInterface } from "node:readline";
import { EventEmitter } from "node:events";
import type { Socket } from "node:net";
import { connect } from "./connection.js";
import { ensureDaemon } from "./daemon.js";
import type { WatchRequest, WatchEvent } from "./types.js";

export interface Watcher extends EventEmitter {
  unsubscribe: () => void;
}

export async function watch(req: WatchRequest): Promise<Watcher> {
  await ensureDaemon();

  const emitter = new EventEmitter() as Watcher;
  let socket: Socket | null = connect();

  socket.on("connect", () => {
    socket?.write(JSON.stringify({ type: "watch", ...req }) + "\n");
  });

  const rl = createInterface({ input: socket });
  rl.on("line", (line) => {
    try {
      const event = JSON.parse(line) as WatchEvent;
      if (event.type === "changed") emitter.emit("change");
    } catch {
      // Ignore malformed lines
    }
  });

  socket.on("error", (err) => emitter.emit("error", err));
  socket.on("close", () => emitter.emit("close"));

  emitter.unsubscribe = () => {
    socket?.destroy();
    socket = null;
  };

  return emitter;
}
