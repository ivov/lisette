import "@fontsource-variable/inter";
import "@fontsource/lexend/latin-600.css";
import "./style.css";
import { setupEditors, GO_PLACEHOLDER } from "./editor/index.js";
import { loadWasmBridge, type Diagnostic, type LisetteBridge } from "./runner/wasm-bridge.js";
import { executeGoSource, formatGoSource } from "./runner/executor.js";
import { readSourceFromHash, copyShareUrl } from "./share.js";
import { EXAMPLES } from "./examples.js";
import type { VimAdapterInstance } from "monaco-vim";

function initResizer() {
  const resizer     = document.getElementById("pane-resizer")!;
  const editorPane  = document.getElementById("editor-pane")!;
  const outputPane  = document.getElementById("output-pane")!;
  const layout      = document.getElementById("main-layout")!;

  let dragging = false;

  resizer.addEventListener("mousedown", (e) => {
    dragging = true;
    resizer.classList.add("dragging");
    e.preventDefault();
  });

  document.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const layoutRect = layout.getBoundingClientRect();
    const totalWidth = layoutRect.width - resizer.offsetWidth;
    const editorWidth = Math.max(200, Math.min(e.clientX - layoutRect.left, totalWidth - 200));
    editorPane.style.flex = "none";
    editorPane.style.width = `${editorWidth}px`;
    outputPane.style.flex = "1";
    outputPane.style.width = "";
  });

  document.addEventListener("mouseup", () => {
    if (!dragging) return;
    dragging = false;
    resizer.classList.remove("dragging");
  });
}

const overlay        = document.getElementById("wasm-loading-overlay")!;
const examplesSelect = document.getElementById("examples") as HTMLSelectElement;
const goPane         = document.getElementById("go-source-editor-container")!;
const btnRun         = document.getElementById("btn-run") as HTMLButtonElement;
const btnFormat      = document.getElementById("btn-format") as HTMLButtonElement;
const btnShare       = document.getElementById("btn-share") as HTMLButtonElement;
const btnVim         = document.getElementById("btn-vim") as HTMLButtonElement;
const vimStatus      = document.getElementById("vim-status")!;
const versionTag     = document.getElementById("brand-version")!;
const statusEl       = document.getElementById("status-indicator")!;
const outputText     = document.getElementById("output-text")!;
const diagnosticList = document.getElementById("diagnostics-list")!;
const outputPane     = document.getElementById("output-pane")!;
const drawerToggle   = document.getElementById("drawer-toggle")!;
const tabBtns        = document.querySelectorAll<HTMLButtonElement>(".tab-btn[data-tab]");
const tabPanels      = document.querySelectorAll<HTMLElement>(".tab-panel");

const NO_COMPILER = '<span class="output-error">The compiler did not load. Reload the page.</span>';

if (/Mac|iPhone|iPad/.test(navigator.platform)) {
  btnRun.title = "Run (⌘+Enter)";
  btnFormat.title = "Format (⇧+⌥+F)";
}

const EXAMPLE_KEY = "lisette:play:example";
const VIM_KEY = "lisette:play:vim";

// Storage is wrapped, because a browser that blocks it must not take the page down with it.
function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

const isMobile = () => window.matchMedia("(max-width: 640px)").matches;

function openDrawer() {
  if (isMobile()) outputPane.classList.add("drawer-open");
}

function toggleDrawer() {
  if (isMobile()) outputPane.classList.toggle("drawer-open");
}

drawerToggle.addEventListener("click", (e) => {
  e.stopPropagation(); // prevent bubbling to output-tabs open handler
  toggleDrawer();
});

document.getElementById("output-tabs")!.addEventListener("click", () => {
  if (!isMobile()) return;
  openDrawer();
});

tabBtns.forEach((btn) => {
  btn.addEventListener("click", () => {
    const target = btn.dataset["tab"]!;
    tabBtns.forEach((b) => b.classList.remove("active"));
    tabPanels.forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.getElementById(`tab-${target}`)?.classList.add("active");
  });
});

function selectTab(name: string) {
  document.querySelector<HTMLButtonElement>(`[data-tab="${name}"]`)?.click();
}

type StatusKind = "idle" | "running" | "ok" | "error" | "warning" | "info";

function setStatus(kind: StatusKind, label: string) {
  statusEl.hidden = false;
  statusEl.className = `status-${kind}`;
  statusEl.textContent = label;
}

function showCounts(diags: Diagnostic[]) {
  const count = (severity: Diagnostic["severity"]) =>
    diags.filter((d) => d.severity === severity).length;
  const errors = count("error");
  const warnings = count("warning");
  const info = count("info");
  if (errors > 0) setStatus("error", `${errors} error${errors === 1 ? "" : "s"}`);
  else if (warnings > 0) setStatus("warning", `${warnings} warning${warnings === 1 ? "" : "s"}`);
  else if (info > 0) setStatus("info", `${info} info`);
  else statusEl.hidden = true;
}

function setOutput(html: string) {
  outputText.innerHTML = html;
  openDrawer();
}

function setButtons(disabled: boolean) {
  btnRun.disabled = disabled;
  btnFormat.disabled = disabled;
  btnShare.disabled = disabled;
}

function renderDiagnostics(diags: Diagnostic[]) {
  if (diags.length === 0) {
    diagnosticList.innerHTML =
      '<p class="diagnostics-empty">All checks passed.</p>';
    return;
  }
  diagnosticList.innerHTML = diags
    .map(
      (d) => `
      <div class="diagnostic-item diagnostic-${d.severity}" data-line="${d.line}" data-col="${d.col}">
        <span class="diagnostic-icon">${d.severity === "error" ? "✕" : d.severity === "warning" ? "▲" : "●"}</span>
        <div>
          <div>${escapeHtml(d.message)}</div>
          ${d.code ? `<div class="diagnostic-location">${escapeHtml(d.code)}</div>` : ""}
          <div class="diagnostic-location">Line ${d.line}, column ${d.col}</div>
        </div>
      </div>`
    )
    .join("");
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

async function main() {
  initResizer();

  const sharedCode = readSourceFromHash();
  for (const example of EXAMPLES) {
    const option = document.createElement("option");
    option.value = example.id;
    option.textContent = example.label;
    examplesSelect.append(option);
  }
  const remembered = readStored(EXAMPLE_KEY);
  const opened = EXAMPLES.find((example) => example.id === remembered) ?? EXAMPLES[0];

  if (sharedCode === null) {
    examplesSelect.value = opened.id;
  } else {
    // Shared code matches no example.
    const shared = document.createElement("option");
    shared.value = "";
    shared.disabled = true;
    shared.selected = true;
    shared.textContent = "Shared code";
    examplesSelect.prepend(shared);
  }

  const editorResult = await setupEditors(
    document.getElementById("editor-container")!,
    goPane,
    sharedCode ?? opened.code,
  );

  let vim: VimAdapterInstance | null = null;
  const setVim = async (on: boolean) => {
    if (on && !vim) {
      const { initVimMode } = await import("monaco-vim");
      if (!vim) vim = initVimMode(editorResult.mainEditor, vimStatus);
    }
    if (!on && vim) {
      vim.dispose();
      vim = null;
    }
    vimStatus.hidden = !on;
    btnVim.setAttribute("aria-pressed", String(on));
    writeStored(VIM_KEY, on ? "on" : "off");
  };
  void setVim(readStored(VIM_KEY) === "on");
  btnVim.addEventListener("click", async () => {
    await setVim(vim === null);
    editorResult.mainEditor.focus();
  });

  const alignStrips = () => {
    const { contentLeft } = editorResult.mainEditor.getLayoutInfo();
    document.documentElement.style.setProperty("--gutter", `${contentLeft}px`);
  };
  alignStrips();
  editorResult.mainEditor.onDidLayoutChange(alignStrips);

  diagnosticList.addEventListener("click", (e) => {
    const item = (e.target as HTMLElement).closest<HTMLElement>(".diagnostic-item");
    if (!item) return;
    const line = parseInt(item.dataset["line"] ?? "1", 10);
    const col  = parseInt(item.dataset["col"] ?? "1", 10);
    editorResult.mainEditor.revealLineInCenter(line);
    editorResult.mainEditor.setPosition({ lineNumber: line, column: col });
    editorResult.mainEditor.focus();
  });

  let bridge = await (async () => {
    try {
      const b = await loadWasmBridge();
      editorResult.setBridge(b);
      return b;
    } catch (err) {
      console.warn("WASM bridge failed to load:", err);
      return null;
    }
  })();

  if (bridge) {
    versionTag.textContent = `v${bridge.version}`;
    versionTag.hidden = false;
  }

  overlay.classList.add("hidden");
  setStatus("idle", "Ready");
  setButtons(false);

  // A run or a format owns the status pill until it finishes.
  let busy = false;
  // Each compile takes a ticket, so a slow result cannot overwrite a newer one.
  let ticket = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async function compileWith(compiler: LisetteBridge) {
    const result = await compiler.compile(editorResult.getCode());
    editorResult.addMarkers(result.diagnostics);
    renderDiagnostics(result.diagnostics);
    return result;
  }

  function settleGo(goSource: string, mine: number) {
    if (mine !== ticket) return;
    editorResult.setGoSource(goSource);
    goPane.classList.remove("is-stale");
  }

  function showGo(goSource: string, mine: number) {
    void formatGoSource(goSource).then((formatted) => settleGo(formatted, mine));
  }

  async function refresh() {
    if (!bridge) return;
    const mine = ++ticket;
    const result = await compileWith(bridge);
    if (mine !== ticket) return;
    if (!busy) showCounts(result.diagnostics);
    if (result.ok && result.goSource) showGo(result.goSource, mine);
    else settleGo(GO_PLACEHOLDER, mine);
  }

  editorResult.mainEditor.onDidChangeModelContent(() => {
    goPane.classList.add("is-stale");
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(refresh, 1000);
  });

  examplesSelect.addEventListener("change", () => {
    const example = EXAMPLES.find((candidate) => candidate.id === examplesSelect.value);
    if (!example) return;
    editorResult.mainEditor.setValue(example.code);
    writeStored(EXAMPLE_KEY, example.id);
    examplesSelect.querySelector('option[value=""]')?.remove();
    if (debounceTimer) clearTimeout(debounceTimer);
    void refresh();
  });

  void refresh();

  btnShare.addEventListener("click", async () => {
    const ok = await copyShareUrl(editorResult.getCode());
    setStatus(ok ? "ok" : "error", ok ? "Link copied" : "Copy failed");
  });

  btnFormat.addEventListener("click", async () => {
    if (!bridge) { setOutput(NO_COMPILER); return; }
    busy = true;
    setButtons(true);
    setStatus("running", "Formatting…");
    const result = await bridge.format(editorResult.getCode());
    if (result.ok) {
      // The format action applies minimal edits, which keeps the caret and the undo history.
      await editorResult.mainEditor.getAction("editor.action.formatDocument")?.run();
      setStatus("ok", "Formatted");
    } else {
      setStatus("error", "Format error");
      setOutput(`<span class="output-error">${escapeHtml(result.error ?? "Unknown error")}</span>`);
    }
    busy = false;
    setButtons(false);
  });

  btnRun.addEventListener("click", run);

  async function run() {
    if (busy) return;
    if (!bridge) {
      setOutput(NO_COMPILER);
      setStatus("error", "No compiler");
      return;
    }
    busy = true;
    setButtons(true);
    setStatus("running", "Compiling…");
    setOutput('<span class="output-hint">Compiling…</span>');

    if (debounceTimer) clearTimeout(debounceTimer);
    const mine = ++ticket;
    const result = await compileWith(bridge);

    if (!result.ok || !result.goSource) {
      settleGo(GO_PLACEHOLDER, mine);
      setOutput('<span class="output-hint">The code did not compile.</span>');
      showCounts(result.diagnostics);
      selectTab("diagnostics");
      busy = false;
      setButtons(false);
      return;
    }

    showGo(result.goSource, mine);
    setStatus("running", "Running…");
    setOutput('<span class="output-hint">Running…</span>');

    const execResult = await executeGoSource(result.goSource);

    if (!execResult.ok) {
      const msg = execResult.error
        ? `<span class="output-error">${escapeHtml(execResult.error)}</span>`
        : `<span class="output-error">${escapeHtml(execResult.stderr)}</span>`;
      setOutput(msg);
      setStatus("error", "Run error");
    } else {
      const out = execResult.stdout || execResult.stderr;
      setOutput(out
        ? escapeHtml(out)
        : '<span class="output-hint">(no output)</span>');
      statusEl.hidden = true;
    }

    selectTab("output");
    busy = false;
    setButtons(false);
  }

  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      run();
    }
  });

  // Expose bridge setter so it can be updated if WASM loads lazily
  (window as unknown as Record<string, unknown>).__lisette_setBridge = (b: typeof bridge) => {
    bridge = b;
    if (b) editorResult.setBridge(b);
  };
}

main().catch(console.error);
