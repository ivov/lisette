import "./style.css";
import { setupEditors, type DiagnosticItem } from "./editor/index.js";
import { loadWasmBridge, type Diagnostic } from "./runner/wasm-bridge.js";
import { executeGoSource, formatGoSource } from "./runner/executor.js";
import { THEME_LIGHT, THEME_DARK } from "./editor/theme.js";
import { readSourceFromHash, writeSourceToHash, copyShareUrl } from "./share.js";

// ─── Pane resizer ─────────────────────────────────────────────────────────────
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

// ─── DOM refs ─────────────────────────────────────────────────────────────────
const overlay        = document.getElementById("wasm-loading-overlay")!;
const btnRun         = document.getElementById("btn-run") as HTMLButtonElement;
const btnFormat      = document.getElementById("btn-format") as HTMLButtonElement;
const btnCheck       = document.getElementById("btn-check") as HTMLButtonElement;
const btnShare       = document.getElementById("btn-share") as HTMLButtonElement;
const versionTag     = document.getElementById("brand-version")!;
const statusEl       = document.getElementById("status-indicator")!;
const outputText     = document.getElementById("output-text")!;
const diagnosticList = document.getElementById("diagnostics-list")!;
const outputPane     = document.getElementById("output-pane")!;
const drawerToggle   = document.getElementById("drawer-toggle")!;
const tabBtns        = document.querySelectorAll<HTMLButtonElement>(".tab-btn[data-tab]");
const tabPanels      = document.querySelectorAll<HTMLElement>(".tab-panel");

// ─── Mobile drawer ─────────────────────────────────────────────────────────────
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

// Tapping the tab strip (but not the chevron) on mobile opens the drawer
document.getElementById("output-tabs")!.addEventListener("click", () => {
  if (!isMobile()) return;
  openDrawer(); // idempotent — adds class if not already present
});

// ─── Tab switching ─────────────────────────────────────────────────────────────
tabBtns.forEach((btn) => {
  btn.addEventListener("click", () => {
    const target = btn.dataset["tab"]!;
    tabBtns.forEach((b) => b.classList.remove("active"));
    tabPanels.forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.getElementById(`tab-${target}`)?.classList.add("active");
  });
});

// ─── Status helpers ───────────────────────────────────────────────────────────
type StatusKind = "idle" | "running" | "ok" | "error" | "warning";

function setStatus(kind: StatusKind, label: string) {
  statusEl.className = `status-${kind}`;
  statusEl.textContent = label;
}

function setOutput(html: string) {
  outputText.innerHTML = html;
  openDrawer();
}

function setButtons(disabled: boolean) {
  btnRun.disabled = disabled;
  btnFormat.disabled = disabled;
  btnCheck.disabled = disabled;
  btnShare.disabled = disabled;
}

// ─── Diagnostics panel ────────────────────────────────────────────────────────
function renderDiagnostics(diags: Diagnostic[]) {
  if (diags.length === 0) {
    diagnosticList.innerHTML =
      '<p class="diagnostics-empty">No diagnostics — your code looks good!</p>';
    return;
  }
  diagnosticList.innerHTML = diags
    .map(
      (d) => `
      <div class="diagnostic-item diagnostic-${d.severity}" data-line="${d.line}" data-col="${d.col}">
        <span class="diagnostic-icon">${d.severity === "error" ? "✖" : d.severity === "warning" ? "⚠" : "ℹ"}</span>
        <div>
          <div>${escapeHtml(d.message)}</div>
          ${d.code ? `<div class="diagnostic-location">${escapeHtml(d.code)}</div>` : ""}
          <div class="diagnostic-location">Line ${d.line}, Col ${d.col}</div>
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

// ─── Bootstrap ────────────────────────────────────────────────────────────────
async function main() {
  initResizer();
  const sharedCode = readSourceFromHash();
  const editorResult = await setupEditors(
    document.getElementById("editor-container")!,
    document.getElementById("go-source-editor-container")!,
    sharedCode ?? undefined,
  );

  // Switch editor theme when the OS colour scheme changes
  const darkMq = window.matchMedia("(prefers-color-scheme: dark)");
  darkMq.addEventListener("change", (e) => {
    editorResult.setTheme(e.matches ? THEME_DARK : THEME_LIGHT);
  });

  // Click on diagnostics jumps to position in editor
  diagnosticList.addEventListener("click", (e) => {
    const item = (e.target as HTMLElement).closest<HTMLElement>(".diagnostic-item");
    if (!item) return;
    const line = parseInt(item.dataset["line"] ?? "1", 10);
    const col  = parseInt(item.dataset["col"] ?? "1", 10);
    editorResult.mainEditor.revealLineInCenter(line);
    editorResult.mainEditor.setPosition({ lineNumber: line, column: col });
    editorResult.mainEditor.focus();
  });

  // ── Load WASM ──────────────────────────────────────────────────────────────
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

  // ── Share ─────────────────────────────────────────────────────────────────
  btnShare.addEventListener("click", async () => {
    writeSourceToHash(editorResult.getCode());
    const ok = await copyShareUrl();
    setStatus(ok ? "ok" : "error", ok ? "Link copied" : "Copy failed");
  });

  // ── Format ────────────────────────────────────────────────────────────────
  btnFormat.addEventListener("click", async () => {
    if (!bridge) { setOutput('<span class="output-error">WASM module not loaded.</span>'); return; }
    setStatus("running", "Formatting…");
    setButtons(true);
    const code = editorResult.getCode();
    const result = await bridge.format(code);
    if (result.ok && result.formatted) {
      editorResult.mainEditor.setValue(result.formatted);
      setStatus("ok", "Formatted");
    } else {
      setStatus("error", "Format error");
      setOutput(`<span class="output-error">${escapeHtml(result.error ?? "Unknown error")}</span>`);
    }
    setButtons(false);
  });

  // ── Type check ────────────────────────────────────────────────────────────
  btnCheck.addEventListener("click", async () => {
    setStatus("running", "Checking…");
    setButtons(true);
    const code = editorResult.getCode();

    let diags: Diagnostic[] = [];
    if (bridge) {
      diags = await bridge.check(code);
    }

    const editorDiags: DiagnosticItem[] = diags.map((d) => ({
      severity: d.severity,
      message: d.message,
      line: d.line,
      col: d.col,
      endLine: d.endLine,
      endCol: d.endCol,
    }));
    editorResult.addMarkers(editorDiags);
    renderDiagnostics(diags);

    // Switch to diagnostics tab and open drawer on mobile
    document.querySelector<HTMLButtonElement>('[data-tab="diagnostics"]')?.click();
    openDrawer();

    const errors = diags.filter((d) => d.severity === "error").length;
    const warnings = diags.filter((d) => d.severity === "warning").length;
    if (errors > 0) setStatus("error", `${errors} error${errors > 1 ? "s" : ""}`);
    else if (warnings > 0) setStatus("warning", `${warnings} warning${warnings > 1 ? "s" : ""}`);
    else setStatus("ok", "No issues");
    setButtons(false);
  });

  // ── Run ───────────────────────────────────────────────────────────────────
  btnRun.addEventListener("click", run);

  async function run() {
    setStatus("running", "Compiling…");
    setButtons(true);
    setOutput('<span class="output-hint">Compiling…</span>');

    const code = editorResult.getCode();

    // 1. Compile Lisette → Go
    let goSource: string;
    if (bridge) {
      const compileResult = await bridge.compile(code);

      // Update diagnostics
      const editorDiags: DiagnosticItem[] = compileResult.diagnostics.map((d) => ({
        severity: d.severity,
        message: d.message,
        line: d.line,
        col: d.col,
        endLine: d.endLine,
        endCol: d.endCol,
      }));
      editorResult.addMarkers(editorDiags);
      renderDiagnostics(compileResult.diagnostics);

      if (!compileResult.ok || !compileResult.goSource) {
        const errors = compileResult.diagnostics
          .filter((d) => d.severity === "error")
          .map((d) => `<span class="output-error">error[${d.code ?? "E"}] at ${d.line}:${d.col}: ${escapeHtml(d.message)}</span>`)
          .join("\n");
        setOutput(errors || '<span class="output-error">Compilation failed.</span>');
        const errCount = compileResult.diagnostics.filter((d) => d.severity === "error").length;
        setStatus("error", `${errCount} error${errCount !== 1 ? "s" : ""}`);
        setButtons(false);
        return;
      }

      goSource = compileResult.goSource;
      // Format the Go output via the Playground /fmt endpoint (best-effort, non-blocking)
      editorResult.setGoSource(await formatGoSource(goSource));
    } else {
      // WASM not available – show stub message
      setOutput('<span class="output-error">WASM compiler not loaded. Run `npm run build:wasm` to build the compiler module.</span>');
      setStatus("error", "No compiler");
      setButtons(false);
      return;
    }

    // 2. Execute Go source via Go Playground API
    setStatus("running", "Running…");
    setOutput('<span class="output-hint">Sending to Go Playground…</span>');

    const execResult = await executeGoSource(goSource);

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
      setStatus("ok", "Done");
    }

    // Switch to output tab
    document.querySelector<HTMLButtonElement>('[data-tab="output"]')?.click();
    setButtons(false);
  }

  // Keyboard shortcut: Ctrl+Enter to run
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      run();
    }
  });

  // Live diagnostics: re-check 1 s after user stops typing
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  editorResult.mainEditor.onDidChangeModelContent(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(async () => {
      if (!bridge) return;
      const code = editorResult.getCode();
      const diags = await bridge.check(code);
      const editorDiags: DiagnosticItem[] = diags.map((d) => ({
        severity: d.severity,
        message: d.message,
        line: d.line,
        col: d.col,
        endLine: d.endLine,
        endCol: d.endCol,
      }));
      editorResult.addMarkers(editorDiags);
    }, 1000);
  });

  // Expose bridge setter so it can be updated if WASM loads lazily
  (window as unknown as Record<string, unknown>).__lisette_setBridge = (b: typeof bridge) => {
    bridge = b;
    if (b) editorResult.setBridge(b);
  };
}

main().catch(console.error);
