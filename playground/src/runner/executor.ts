/**
 * Executes compiled Go source via the Go Playground API.
 *
 * Flow:
 *   Lisette source → WASM compile → Go source → Go Playground → stdout/stderr
 */

const GO_PLAYGROUND_API = "https://play.golang.org/compile";
const GO_PLAYGROUND_FMT = "https://play.golang.org/fmt";

/** Format Go source via the Go Playground /fmt endpoint. Returns the original on any failure. */
export async function formatGoSource(goSource: string): Promise<string> {
  try {
    const response = await fetch(GO_PLAYGROUND_FMT, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ body: goSource }).toString(),
    });
    if (!response.ok) return goSource;
    const data = (await response.json()) as { Body?: string; Error?: string };
    return data.Body && !data.Error ? data.Body : goSource;
  } catch {
    return goSource;
  }
}

export interface ExecuteResult {
  stdout: string;
  stderr: string;
  ok: boolean;
  error?: string;
}

/** Send Go source to the Go Playground and return the output. */
export async function executeGoSource(goSource: string): Promise<ExecuteResult> {
  const body = new URLSearchParams({
    version: "2",
    body: goSource,
    withVet: "true",
  });

  let response: Response;
  try {
    response = await fetch(GO_PLAYGROUND_API, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: body.toString(),
    });
  } catch (networkError) {
    return {
      stdout: "",
      stderr: "",
      ok: false,
      error: `Network error: ${String(networkError)}. Is the Go Playground reachable?`,
    };
  }

  if (!response.ok) {
    return {
      stdout: "",
      stderr: "",
      ok: false,
      error: `Go Playground returned HTTP ${response.status}`,
    };
  }

  // Response shape: { Errors: string; Events: [{Message: string; Kind: "stdout"|"stderr"; Delay: number}]; IsTest: bool; Status: number; TestsFailed: number; VetOK: bool }
  interface PlaygroundEvent {
    Message: string;
    Kind: "stdout" | "stderr";
    Delay: number;
  }
  interface PlaygroundResponse {
    Errors: string;
    Events: PlaygroundEvent[] | null;
    Status: number;
  }

  const data = (await response.json()) as PlaygroundResponse;

  if (data.Errors) {
    return { stdout: "", stderr: data.Errors, ok: false };
  }

  const stdout = (data.Events ?? [])
    .filter((e) => e.Kind === "stdout")
    .map((e) => e.Message)
    .join("");
  const stderr = (data.Events ?? [])
    .filter((e) => e.Kind === "stderr")
    .map((e) => e.Message)
    .join("");

  return { stdout, stderr, ok: data.Status === 0 };
}
