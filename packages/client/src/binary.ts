import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { join } from "node:path";

const require = createRequire(import.meta.url);

type PlatformPackage = {
  packageName: string;
  binaryName: string;
};

function getPlatformPackage(): PlatformPackage {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "darwin") {
    if (arch === "arm64") {
      return { packageName: "@fs-hasher/darwin-arm64", binaryName: "fs-hasher" };
    }
    if (arch === "x64") {
      return { packageName: "@fs-hasher/darwin-x64", binaryName: "fs-hasher" };
    }
  }

  if (platform === "linux") {
    const isMusl = isMuslLibc();

    if (arch === "arm64") {
      return isMusl
        ? { packageName: "@fs-hasher/linux-arm64-musl", binaryName: "fs-hasher" }
        : { packageName: "@fs-hasher/linux-arm64-gnu", binaryName: "fs-hasher" };
    }
    if (arch === "x64") {
      return isMusl
        ? { packageName: "@fs-hasher/linux-x64-musl", binaryName: "fs-hasher" }
        : { packageName: "@fs-hasher/linux-x64-gnu", binaryName: "fs-hasher" };
    }
    if (arch === "arm") {
      return { packageName: "@fs-hasher/linux-arm-gnueabihf", binaryName: "fs-hasher" };
    }
  }

  if (platform === "win32") {
    if (arch === "x64") {
      return { packageName: "@fs-hasher/win32-x64-msvc", binaryName: "fs-hasher.exe" };
    }
    if (arch === "arm64") {
      return { packageName: "@fs-hasher/win32-arm64-msvc", binaryName: "fs-hasher.exe" };
    }
    if (arch === "ia32") {
      return { packageName: "@fs-hasher/win32-ia32-msvc", binaryName: "fs-hasher.exe" };
    }
  }

  throw new Error(`Unsupported platform: ${platform}-${arch}`);
}

function isMuslLibc(): boolean {
  // Check for Alpine or musl-based distros
  try {
    const { execSync } = require("node:child_process") as typeof import("node:child_process");
    const output = execSync("ldd --version 2>&1", { encoding: "utf8" });
    return output.toLowerCase().includes("musl");
  } catch {
    // If ldd fails, check /etc/os-release for Alpine
    try {
      const fs = require("node:fs") as typeof import("node:fs");
      const osRelease = fs.readFileSync("/etc/os-release", "utf8");
      return osRelease.toLowerCase().includes("alpine");
    } catch {
      return false;
    }
  }
}

export function getBinaryPath(): string {
  const { packageName, binaryName } = getPlatformPackage();

  try {
    const packagePath = require.resolve(`${packageName}/package.json`);
    const packageDir = packagePath.replace(/package\.json$/, "");
    const binaryPath = join(packageDir, "bin", binaryName);

    if (existsSync(binaryPath)) {
      return binaryPath;
    }
  } catch {
    // Package not installed
  }

  throw new Error(
    `Could not find fs-hasher binary for ${process.platform}-${process.arch}. ` +
    `Try reinstalling the package or ensure ${packageName} is installed.`
  );
}
