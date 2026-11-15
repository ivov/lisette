// Usage: node scripts/lsp-smoke.mjs <directory with lisette_wasm.js and lisette_wasm_bg.wasm>

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "../src/runner/lsp-client.ts";

const dir = resolve(process.argv[2] ?? "public/wasm");
const wasmBytes = readFileSync(join(dir, "lisette_wasm_bg.wasm"));
const glueUrl = pathToFileURL(join(dir, "lisette_wasm.js")).href;

async function loadSession(instance) {
  const glue = await import(`${glueUrl}?instance=${instance}`);
  glue.initSync({ module: wasmBytes });
  return new glue.LspSession();
}

function start(text, session) {
  const published = [];
  const client = new LspClient(
    session,
    text,
    (diagnostics) => published.push(diagnostics),
    loadSession,
  );
  return { client, published };
}

function at(line, character) {
  return {
    textDocument: { uri: "file:///playground/playground.lis" },
    position: { line, character },
  };
}

const tests = [];
const test = (name, run) => tests.push({ name, run });

test("hover after non-ASCII text uses UTF-16 positions", async () => {
  const source = 'fn main() {\n  let label = "Grüße aus Köln 🎉"\n  let size = label.length()\n  let _ = size\n}\n';
  const { client } = start(source, await loadSession(1));
  const hover = client.request("textDocument/hover", at(2, 15));
  assert.equal(hover.contents.value, "```lisette\nstring\n```");
  assert.deepEqual(hover.range, {
    start: { line: 2, character: 13 },
    end: { line: 2, character: 18 },
  });
});

test("completion right after a dot offers the fields", async () => {
  const source = "struct Point { x: int, y: int }\n\nfn main() {\n  let p = Point { x: 1, y: 2 }\n  let _ = p.\n}\n";
  const { client } = start(source, await loadSession(2));
  const items = client.request("textDocument/completion", at(4, 12));
  const labels = items.map((item) => item.label);
  assert.ok(labels.includes("x") && labels.includes("y"), `labels: ${labels}`);
});

test("diagnostics after non-ASCII text use UTF-16 columns", async () => {
  const source = 'fn main() {\n  let label = "Grüße aus Köln 🎉"; let size: string = 1\n  let _ = size\n  let _ = label\n}\n';
  const { published } = start(source, await loadSession(3));
  const diagnostics = published.at(-1);
  assert.equal(diagnostics.length, 1);
  assert.deepEqual(diagnostics[0].range.start, { line: 1, character: 54 });
});

test("a request sends the pending edit first", async () => {
  const { client, published } = start("fn main() {}\n", await loadSession(4));
  client.change('fn main() {\n  let word = "é"\n  let _ = word\n}\n');
  const hover = client.request("textDocument/hover", at(1, 7));
  assert.equal(hover.contents.value, "```lisette\nstring\n```");
  assert.deepEqual(published.at(-1), []);
});

test("the client restarts in a new instance after the session stops", async () => {
  const stopped = {
    calls: 0,
    handle(message) {
      if (++this.calls > 3) throw new WebAssembly.RuntimeError("unreachable");
      return "[]";
    },
    free() {},
  };
  const { client } = start("fn main() {\n  let n = 1\n  let _ = n\n}\n", stopped);
  assert.equal(client.request("textDocument/hover", at(1, 6)), null);
  await new Promise((done) => setTimeout(done, 0));
  const hover = client.request("textDocument/hover", at(1, 6));
  assert.equal(hover.contents.value, "```lisette\nint\n```");
});

let failed = 0;
for (const { name, run } of tests) {
  try {
    await run();
    console.log(`ok - ${name}`);
  } catch (err) {
    failed++;
    console.log(`not ok - ${name}\n${err.stack}`);
  }
}
process.exit(failed === 0 ? 0 : 1);
