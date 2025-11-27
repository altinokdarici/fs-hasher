import { request } from "./connection.js";
import { ensureDaemon } from "./daemon.js";
import type { HashRequest, HashResult } from "./types.js";

export async function hash(req: HashRequest): Promise<HashResult> {
  await ensureDaemon();
  return request<HashResult>({
    type: "hash",
    ...req,
  });
}
