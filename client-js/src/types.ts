export interface HashRequest {
  root: string;
  path: string;
  glob: string;
  persistent?: boolean;
}

export interface HashResult {
  hash: string;
  file_count: number;
}

export interface WatchRequest {
  root: string;
  path: string;
  glob: string;
}

export interface WatchEvent {
  type: "changed";
}

export class FswatchdError extends Error {
  constructor(
    message: string,
    public readonly code?: string
  ) {
    super(message);
    this.name = "FswatchdError";
  }
}
