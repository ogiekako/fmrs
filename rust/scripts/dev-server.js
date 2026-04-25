#!/usr/bin/env node

const { spawn } = require("child_process");
const http = require("http");
const path = require("path");

const root = path.resolve(__dirname, "../..");
const children = [];
let shuttingDown = false;

function spawnChild(command, args, cwd) {
  const child = spawn(command, args, {
    cwd,
    stdio: "inherit",
    shell: false,
  });
  children.push(child);

  child.on("exit", (code, signal) => {
    if (shuttingDown) {
      return;
    }
    if (signal || code !== 0) {
      shutdown();
      process.exit(code ?? 1);
    }
  });

  return child;
}

async function main() {
  if (await isFmrsServerAlive()) {
    console.error("Using existing fmrs server on http://localhost:1234");
  } else {
    spawnChild("cargo", ["run", "-r", "--", "server"], path.join(root, "rust"));
  }
  spawnChild("npm", ["run", "serve"], root);
}

function isFmrsServerAlive() {
  return new Promise((resolve) => {
    let resolved = false;
    const finish = (alive) => {
      if (!resolved) {
        resolved = true;
        resolve(alive);
      }
    };
    const req = http.get("http://127.0.0.1:1234/fmrs_alive", (res) => {
      res.resume();
      finish(res.statusCode === 200);
    });
    req.on("error", () => finish(false));
    req.setTimeout(500, () => {
      req.destroy();
      finish(false);
    });
  });
}

function shutdown() {
  shuttingDown = true;
  for (const child of children) {
    if (!child.killed) {
      child.kill("SIGTERM");
    }
  }
}

process.on("SIGINT", () => {
  shutdown();
  process.exit(130);
});

process.on("SIGTERM", () => {
  shutdown();
  process.exit(143);
});

main().catch((e) => {
  console.error(e);
  shutdown();
  process.exit(1);
});
