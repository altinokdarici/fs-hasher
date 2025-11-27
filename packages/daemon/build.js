#!/usr/bin/env node

/**
 * Build script for fs-hasher daemon.
 *
 * Usage:
 *   node build.js                    # Local: build for current platform
 *   node build.js <rust-target>      # CI: build for specific target
 *
 * Rust targets:
 *   - aarch64-apple-darwin       -> @fs-hasher/darwin-arm64
 *   - x86_64-apple-darwin        -> @fs-hasher/darwin-x64
 *   - x86_64-unknown-linux-gnu   -> @fs-hasher/linux-x64-gnu
 *   - x86_64-unknown-linux-musl  -> @fs-hasher/linux-x64-musl
 *   - aarch64-pc-windows-msvc    -> @fs-hasher/win32-arm64-msvc
 *   - x86_64-pc-windows-msvc     -> @fs-hasher/win32-x64-msvc
 */

import { execSync } from "node:child_process";
import { cpSync, mkdirSync, chmodSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const DAEMON_DIR = __dirname;
const PACKAGES_DIR = join(__dirname, "..");

const TARGET_MAP = {
  // Rust target -> npm package name
  "aarch64-apple-darwin": "darwin-arm64",
  "x86_64-apple-darwin": "darwin-x64",
  "x86_64-unknown-linux-gnu": "linux-x64-gnu",
  "x86_64-unknown-linux-musl": "linux-x64-musl",
  "aarch64-pc-windows-msvc": "win32-arm64-msvc",
  "x86_64-pc-windows-msvc": "win32-x64-msvc",
};

// Node platform/arch -> Rust target (for local builds)
const LOCAL_TARGET_MAP = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "win32-x64": "x86_64-pc-windows-msvc",
};

function getLocalTarget() {
  const key = `${process.platform}-${process.arch}`;
  const target = LOCAL_TARGET_MAP[key];
  if (!target) {
    console.error(`Unsupported local platform: ${key}`);
    process.exit(1);
  }
  return target;
}

function main() {
  const target = process.argv[2] ?? getLocalTarget();
  const isLocalBuild = !process.argv[2];

  const packageName = TARGET_MAP[target];
  if (!packageName) {
    console.error(`Unknown target: ${target}`);
    console.error("\nAvailable targets:");
    for (const t of Object.keys(TARGET_MAP)) {
      console.error(`  ${t}`);
    }
    process.exit(1);
  }

  const isWindows = target.includes("windows");
  const binaryName = isWindows ? "fs-hasher.exe" : "fs-hasher";

  // Build
  if (isLocalBuild) {
    console.log(`Building daemon for ${target}...`);
    execSync("cargo build --release", { cwd: DAEMON_DIR, stdio: "inherit" });
  } else {
    console.log(`Building daemon for ${target} (cross-compile)...`);
    execSync(`cargo build --release --target ${target}`, { cwd: DAEMON_DIR, stdio: "inherit" });
  }

  // Copy binary to package
  const srcDir = isLocalBuild
    ? join(DAEMON_DIR, "target", "release")
    : join(DAEMON_DIR, "target", target, "release");
  const src = join(srcDir, binaryName);

  const destDir = join(PACKAGES_DIR, packageName, "bin");
  const dest = join(destDir, binaryName);

  mkdirSync(destDir, { recursive: true });
  cpSync(src, dest);

  if (!isWindows) {
    chmodSync(dest, 0o755);
  }

  console.log(`\nBuilt: packages/${packageName}/bin/${binaryName}`);
}

main();
