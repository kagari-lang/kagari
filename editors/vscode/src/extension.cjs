'use strict';

const vscode = require('vscode');
const { ClientController } = require('./controller.cjs');
const { createClient } = require('./client.cjs');

let controller;

function activate(context) {
  const output = vscode.window.createOutputChannel('Kagari Language Server', { log: true });
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 10);
  const changed = new vscode.EventEmitter();
  status.command = 'kagari.showOutput';
  status.show();
  controller = new ClientController({
    create: (configuration, closed) => createClient(configuration, closed, output),
    report: (message) => output.appendLine(message),
    changed: (value) => {
      status.text = `Kagari: ${value.state}`;
      status.tooltip = value.error ?? value.path ?? 'Kagari language server';
      changed.fire(snapshot());
    },
  });
  function snapshot() {
    // Consumers can inspect lifecycle state, never mutate or send through the client.
    return Object.freeze(JSON.parse(JSON.stringify(controller.status)));
  }
  function configuration() {
    const settings = vscode.workspace.getConfiguration('kagari.server');
    return { enabled: settings.get('enabled', true), path: settings.get('path', ''),
      standardSources: context.asAbsolutePath('dist/stdlib') };
  }
  context.subscriptions.push(output, status, changed,
    vscode.commands.registerCommand('kagari.restartServer', () => controller.restart()),
    vscode.commands.registerCommand('kagari.showOutput', () => output.show(true)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('kagari.server')) void controller.configure(configuration());
    }));
  void controller.configure(configuration());
  return Object.freeze({ get status() { return snapshot(); }, onDidChangeStatus: changed.event });
}

async function deactivate() {
  await controller?.dispose();
}

module.exports = { activate, deactivate };
