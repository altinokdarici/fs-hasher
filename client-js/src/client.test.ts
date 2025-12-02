import { describe, it, before, after } from "node:test";
import assert from "node:assert";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client, ensureDaemon } from "./index.js";

describe("fswatchd client", () => {
  let testDir: string;
  let client: Client;

  before(async () => {
    testDir = await mkdtemp(join(tmpdir(), "fswatchd-test-"));
    await ensureDaemon();
    client = new Client();
    await client.connect();
  });

  after(async () => {
    client.close();
    await rm(testDir, { recursive: true, force: true });
  });

  describe("hash", () => {
    it("should hash a single file", async () => {
      const filePath = join(testDir, "test.txt");
      await writeFile(filePath, "hello world");

      const result = await client.hash({
        root: testDir,
        path: ".",
        glob: "*.txt",
      });

      assert.ok(result.hash, "should return a hash");
      assert.strictEqual(result.file_count, 1);
    });

    it("should hash multiple files", async () => {
      await writeFile(join(testDir, "a.txt"), "file a");
      await writeFile(join(testDir, "b.txt"), "file b");

      const result = await client.hash({
        root: testDir,
        path: ".",
        glob: "**/*.txt",
      });

      assert.ok(result.hash);
      assert.strictEqual(result.file_count, 3); // test.txt + a.txt + b.txt
    });

    it("should return different hash for different content", async () => {
      await writeFile(join(testDir, "dir1.txt"), "content 1");

      const result1 = await client.hash({
        root: testDir,
        path: ".",
        glob: "dir1.txt",
      });

      await writeFile(join(testDir, "dir2.txt"), "content 2");

      const result2 = await client.hash({
        root: testDir,
        path: ".",
        glob: "dir2.txt",
      });

      assert.notStrictEqual(result1.hash, result2.hash);
    });

    it("should return same hash for same content", async () => {
      await writeFile(join(testDir, "same1.txt"), "identical");

      const result1 = await client.hash({
        root: testDir,
        path: ".",
        glob: "same1.txt",
      });

      await writeFile(join(testDir, "same2.txt"), "identical");

      const result2 = await client.hash({
        root: testDir,
        path: ".",
        glob: "same2.txt",
      });

      assert.strictEqual(result1.hash, result2.hash);
    });
  });

  describe("errors", () => {
    it("should error on non-existent root", async () => {
      await assert.rejects(
        client.hash({ root: "/nonexistent/path", path: ".", glob: "*.txt" }),
        /error/i
      );
    });

    it("should error when no files match glob", async () => {
      await assert.rejects(
        client.hash({ root: testDir, path: ".", glob: "*.nonexistent" }),
        /no files/i
      );
    });
  });

  describe("watch", () => {
    it("should create watcher and allow unsubscribe", async () => {
      const watcher = await client.watch(
        {
          root: testDir,
          path: ".",
          glob: "**/*",
        },
        (_paths) => {
          // callback for file changes
        }
      );

      assert.ok(watcher, "should return a watcher");
      assert.ok(watcher.key, "should have a subscription key");
      assert.ok(typeof watcher.unsubscribe === "function", "should have unsubscribe method");

      await watcher.unsubscribe();
    });

    it("should detect file changes", { timeout: 5000 }, async () => {
      // Use a unique subdirectory to avoid interference from earlier tests
      const { mkdirSync } = await import("node:fs");
      const watchSubDir = join(testDir, "watch-subdir");
      mkdirSync(watchSubDir, { recursive: true });
      const watchFile = join(watchSubDir, "watch-test.txt");

      let onChangeCallback: (paths: string[]) => void;
      const firstChange = new Promise<string[]>((resolve) => {
        onChangeCallback = resolve;
      });

      const watcher = await client.watch(
        {
          root: watchSubDir,
          path: ".",
          glob: "**/*.txt",
        },
        (paths) => onChangeCallback(paths)
      );

      // Write repeatedly until FSEvents delivers (it has inherent startup latency)
      const writeLoop = async () => {
        for (let i = 0; i < 50; i++) {
          await writeFile(watchFile, `content ${i}`);
          await new Promise((r) => setTimeout(r, 20));
        }
      };
      writeLoop(); // don't await - let it run in background

      const changedPaths = await firstChange;

      assert.ok(changedPaths.length > 0, "should have received change events");
      assert.ok(
        changedPaths.some((p: string) => p.includes("watch-test.txt")),
        `should include the changed file path, got: ${JSON.stringify(changedPaths)}`
      );

      await watcher.unsubscribe();
    });
  });
});
