<p align="center">
  <h1 align="center">fs-hasher</h1>
  <p align="center">A fast file hashing daemon with instant lookups</p>
</p>

## Purpose

fs-hasher watches directories and maintains content hashes of files. After the first hash computation, subsequent lookups are instant. File changes are detected automatically and the cache stays up-to-date.

## What it does

- Computes xxHash (xxh3) of files matching glob patterns
- Caches individual file hashes in memory
- Watches for file changes and invalidates cache automatically
- Persists watch roots across daemon restarts

## Usage

Communication via Unix socket (`/tmp/fs-hasher.sock`) or Windows named pipe (`\\.\pipe\fs-hasher`):

### Hash request

```json
{"cmd":"hash","root":"/my/project","path":"src","glob":"*.rs","persistent":true}
```

Response:
```json
{"hash":"5c5f87e433151544","file_count":4}
```

- `persistent: true` - starts file watcher, caches results, survives daemon restart
- `persistent: false` (default) - one-shot hash, no caching

### Watch request

```json
{"cmd":"watch","root":"/my/project","path":"src","glob":"*.rs"}
```

Keeps connection open. Sends events when matching files change:
```json
{"event":"changed","paths":["/my/project/src/main.rs"]}
```

### Unwatch request

```json
{"cmd":"unwatch","root":"/my/project","path":"src","glob":"*.rs"}
```

## How it works

1. **First call**: walks directory, hashes all matching files, stores in cache
2. **Subsequent calls**: returns cached aggregate hash instantly
3. **File changes**: watcher detects change, invalidates cache for that file
4. **Daemon restart**: loads persisted watch roots, re-hashes in background

## Building

```bash
cd fswatchd
cargo build --release
```

## Protocol

Newline-delimited JSON (NDJSON) over:
- Unix/macOS: `/tmp/fs-hasher.sock`
- Windows: `\\.\pipe\fs-hasher`

## License

MIT
