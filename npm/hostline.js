#!/usr/bin/env node
// `npx hostline 5173`: runs the Hostline binary for this system. The binary comes from the GitHub
// release that matches this package's version, downloaded once and kept in the user's cache.
// npm picks the version, so the binary never updates itself here (HOSTLINE_NO_UPDATE).

import { spawn } from "node:child_process";
import { createWriteStream, existsSync, mkdirSync, readFileSync, renameSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const { version } = JSON.parse(readFileSync(new URL("package.json", import.meta.url), "utf8"));

// The release's file for each system (named by .github/workflows/release.yml). Windows on Arm
// runs the x86_64 build.
const ASSETS = {
  "linux x64": "hostline-linux-x86_64",
  "linux arm64": "hostline-linux-arm64",
  "darwin x64": "hostline-macos",
  "darwin arm64": "hostline-macos",
  "win32 x64": "hostline-windows-x86_64.exe",
  "win32 arm64": "hostline-windows-x86_64.exe",
};
const asset = ASSETS[`${process.platform} ${process.arch}`];
if (!asset) {
  console.error(
    `hostline: there's no build for ${process.platform} ${process.arch} yet. With Rust: cargo install --git https://github.com/luannzin/hostline`,
  );
  process.exit(1);
}

const cache =
  process.platform === "win32"
    ? (process.env.LOCALAPPDATA ?? join(homedir(), "AppData", "Local"))
    : process.platform === "darwin"
      ? join(homedir(), "Library", "Caches")
      : (process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache"));
const dir = join(cache, "hostline", version);
const bin = join(dir, process.platform === "win32" ? "hostline.exe" : "hostline");

if (!existsSync(bin)) {
  mkdirSync(dir, { recursive: true });
  const url = `https://github.com/luannzin/hostline/releases/download/v${version}/${asset}`;
  console.error(`Downloading Hostline ${version}...`);
  const res = await fetch(url);
  if (!res.ok || !res.body) {
    console.error(`hostline: couldn't download ${url} (${res.status})`);
    process.exit(1);
  }
  // written under another name first, so a download cut short never runs
  const part = `${bin}.${process.pid}.part`;
  await pipeline(Readable.fromWeb(res.body), createWriteStream(part, { mode: 0o755 }));
  renameSync(part, bin);
}

const child = spawn(bin, process.argv.slice(2), {
  stdio: "inherit",
  env: { ...process.env, HOSTLINE_NO_UPDATE: "1" },
});
// Hostline tells open pages the link ended, then exits, and so do we. In a terminal Ctrl+C
// reaches it anyway; passing the signal on covers a signal sent to this process alone.
for (const signal of ["SIGINT", "SIGTERM"])
  process.on(signal, () => child.kill(signal));
child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
