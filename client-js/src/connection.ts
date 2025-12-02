import { createConnection, type Socket } from "node:net";
import { FswatchdError } from "./types.js";

export const SOCKET_PATH =
  process.platform === "win32"
    ? "\\\\.\\pipe\\fswatchd"
    : "/tmp/fswatchd.sock";

/** Check if daemon is reachable */
export function isConnectable(): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = createConnection(SOCKET_PATH);
    socket.on("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.on("error", () => resolve(false));
  });
}

/** Callback type for subscription events */
export type SubscriptionCallback = (paths: string[]) => void;

/**
 * Client connection to the daemon.
 * Supports multiple hash requests and watch subscriptions over a single connection.
 *
 * @example
 * ```ts
 * const client = new Client();
 * await client.connect();
 *
 * // Multiple hash requests
 * const result1 = await client.hash({ root: "/repo", path: "src", glob: "*.ts" });
 * const result2 = await client.hash({ root: "/repo", path: "lib", glob: "*.ts" });
 *
 * // Multiple watch subscriptions
 * const sub1 = await client.watch({ root: "/repo", path: "src", glob: "*.ts" }, (paths) => {
 *   console.log("src changed:", paths);
 * });
 * const sub2 = await client.watch({ root: "/repo", path: "lib", glob: "*.ts" }, (paths) => {
 *   console.log("lib changed:", paths);
 * });
 *
 * // Unsubscribe individually
 * await sub1.unsubscribe();
 *
 * // Close when done
 * client.close();
 * ```
 */
export class Client {
  private socket: Socket | null = null;
  private buffer = "";
  private responseQueue: Array<{
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }> = [];
  private subscriptions = new Map<string, SubscriptionCallback>();
  private connectPromise: Promise<void> | null = null;
  private closed = false;

  /** Connect to the daemon */
  connect(): Promise<void> {
    if (this.connectPromise) return this.connectPromise;
    if (this.closed) return Promise.reject(new FswatchdError("Client closed"));

    this.connectPromise = new Promise((resolve, reject) => {
      this.socket = createConnection(SOCKET_PATH);

      this.socket.on("connect", () => resolve());

      this.socket.on("error", (err) => {
        if (!this.connectPromise) return;
        this.connectPromise = null;
        reject(new FswatchdError(`Connection failed: ${err.message}`, "ECONNREFUSED"));
      });

      this.socket.on("data", (data) => this.handleData(data));

      this.socket.on("close", () => {
        this.connectPromise = null;
        this.socket = null;
        // Reject pending requests
        for (const { reject } of this.responseQueue) {
          reject(new FswatchdError("Connection closed"));
        }
        this.responseQueue = [];
      });
    });

    return this.connectPromise;
  }

  /** Handle incoming data from socket */
  private handleData(data: Buffer): void {
    this.buffer += data.toString();

    const lines = this.buffer.split("\n");
    this.buffer = lines.pop() ?? "";

    for (const line of lines) {
      if (!line.trim()) continue;

      try {
        const msg = JSON.parse(line) as Record<string, unknown>;

        // If waiting for a response, first line is the response
        const pending = this.responseQueue[0];
        if (pending) {
          this.responseQueue.shift();
          if ("error" in msg) {
            pending.reject(new FswatchdError(msg["error"] as string));
          } else {
            pending.resolve(msg);
          }
        } else if ("key" in msg && "paths" in msg) {
          // Subscription event (only when not waiting for response)
          const callback = this.subscriptions.get(msg["key"] as string);
          if (callback) {
            callback(msg["paths"] as string[]);
          }
        }
      } catch {
        // Ignore malformed JSON
      }
    }
  }

  /** Send a request and wait for response */
  private async request<T>(cmd: Record<string, unknown>): Promise<T> {
    await this.connect();

    return new Promise((resolve, reject) => {
      this.responseQueue.push({
        resolve: resolve as (value: unknown) => void,
        reject,
      });

      const line = JSON.stringify(cmd) + "\n";
      this.socket!.write(line);
    });
  }

  /** Hash files matching a glob pattern */
  async hash(req: {
    root: string;
    path: string;
    glob: string;
    persistent?: boolean;
  }): Promise<{ hash: string; file_count: number }> {
    return this.request({
      cmd: "hash",
      root: req.root,
      path: req.path,
      glob: req.glob,
      persistent: req.persistent ?? false,
    });
  }

  /** Watch for file changes matching a glob pattern */
  async watch(
    req: { root: string; path: string; glob: string },
    callback: SubscriptionCallback
  ): Promise<{ key: string; unsubscribe: () => Promise<void> }> {
    const response = await this.request<{ key: string }>({
      cmd: "watch",
      root: req.root,
      path: req.path,
      glob: req.glob,
    });

    this.subscriptions.set(response.key, callback);

    return {
      key: response.key,
      unsubscribe: async () => {
        this.subscriptions.delete(response.key);
        await this.request({ cmd: "unwatch", key: response.key });
      },
    };
  }

  /** Close the connection */
  close(): void {
    this.closed = true;
    this.socket?.destroy();
    this.socket = null;
    this.connectPromise = null;
    this.subscriptions.clear();
  }
}
