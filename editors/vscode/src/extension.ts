import { type ExtensionContext, window, workspace } from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("lisette");
  const command: string = config.get("serverPath") || "lis";

  const serverOptions: ServerOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "lisette" }],
  };

  client = new LanguageClient(
    "lisette",
    "Lisette",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
  } catch {
    window.showErrorMessage(
      `Failed to find the Lisette language server. Install it with "cargo install lisette" or configure "lisette.serverPath" in VS Code settings to point to your binary.`,
    );
  }
}

export async function deactivate() {
  await client?.stop();
}
