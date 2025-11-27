import { describe, it, before, after } from "node:test";
import assert from "node:assert";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { hash, watch } from "./index.js";

describe("fswatchd client", () => {
  let testDir: string;

  before(async () => {
    testDir = await mkdtemp(join(tmpdir(), "fswatchd-test-"));
  });

  after(async () => {
    await rm(testDir, { recursive: true, force: true });
  });

  describe("hash", () => {
    it("should hash a single file", async () => {
      const filePath = join(testDir, "test.txt");
      await writeFile(filePath, "hello world");

      const result = await hash({
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

      const result = await hash({
        root: testDir,
        path: ".",
        glob: "*.txt",
      });

      assert.ok(result.hash);
      assert.strictEqual(result.file_count, 3); // test.txt + a.txt + b.txt
    });

    it("should return different hash for different content", async () => {
      await writeFile(join(testDir, "dir1.txt"), "content 1");

      const result1 = await hash({
        root: testDir,
        path: ".",
        glob: "dir1.txt",
      });

      await writeFile(join(testDir, "dir2.txt"), "content 2");

      const result2 = await hash({
        root: testDir,
        path: ".",
        glob: "dir2.txt",
      });

      assert.notStrictEqual(result1.hash, result2.hash);
    });

    it("should return same hash for same content", async () => {
      await writeFile(join(testDir, "same1.txt"), "identical");

      const result1 = await hash({
        root: testDir,
        path: ".",
        glob: "same1.txt",
      });

      await writeFile(join(testDir, "same2.txt"), "identical");

      const result2 = await hash({
        root: testDir,
        path: ".",
        glob: "same2.txt",
      });

      assert.strictEqual(result1.hash, result2.hash);
    });
  });

  describe("watch", () => {
    it("should create watcher and allow unsubscribe", async () => {
      const watcher = await watch({
        root: testDir,
        path: ".",
        glob: "**/*",
      });

      assert.ok(watcher, "should return a watcher");
      assert.ok(typeof watcher.on === "function", "should have on method");
      assert.ok(typeof watcher.unsubscribe === "function", "should have unsubscribe method");

      watcher.unsubscribe();
    });
  });
});
