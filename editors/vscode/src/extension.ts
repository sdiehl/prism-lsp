import { ExtensionContext, workspace } from "vscode";
import { LanguageClient, LanguageClientOptions, ServerOptions } from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  const command = workspace.getConfiguration("prism").get<string>("server.path", "prism-lsp");
  const server: ServerOptions = { command };
  const options: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "prism" }],
  };
  client = new LanguageClient("prism", "Prism", server, options);
  context.subscriptions.push(client);
  await client.start();
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}
