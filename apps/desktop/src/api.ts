// The only module that talks to the Tauri runtime. Every request and every
// event subscription goes through here, so changing how the backend is
// reached, or how its updates arrive, touches this file and nothing else.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Bindings,
  ErrorCode,
  Installation,
  Launch,
  LaunchAction,
  Session,
  Snapshot,
} from "./contract";

export const api = {
  snapshot: (live: boolean) => invoke<Snapshot>("snapshot", { live }),
  setDefault: (account: string) => invoke<Snapshot>("set_default", { account }),
  bindings: (directory: string | null) =>
    invoke<Bindings>("list_bindings", { directory }),
  bind: (directory: string, account: string | null) =>
    invoke<void>("set_binding", { directory, account }),
  sessions: () => invoke<Session[]>("list_sessions"),
  terminal: (account: string, directory: string | null, action: LaunchAction) =>
    invoke<Launch>("open_terminal", { account, directory, action }),
  installation: () => invoke<Installation>("installation"),
  chooseDirectory: () => invoke<string | null>("choose_directory"),
};

export type Unsubscribe = () => void;

function on<T>(event: string, handler: (payload: T) => void): Unsubscribe {
  const stop = listen<T>(event, (e) => handler(e.payload));
  return () => void stop.then((unlisten) => unlisten()).catch(() => {});
}

/**
 * What the backend pushes without being asked: the menu bar refreshes and
 * navigates with the window closed. Each returns its own unsubscribe, which
 * is what an effect's cleanup wants.
 */
export const events = {
  snapshot: (handler: (snapshot: Snapshot) => void) => on("snapshot", handler),
  navigate: (handler: (page: string) => void) => on("navigate", handler),
  backendError: (handler: (code: unknown) => void) =>
    on("backend-error", handler),
};

export const errors: Record<ErrorCode, string> = {
  identity_drift:
    "The account identity differs from its pin, or the pin is corrupt. Inspect it with the CLI before launching.",
  shared_credential_store:
    "A shared credential-store override is set. Live refresh is disabled because it could report one account for every directory. Launch the manager without CLAUDE_SECURESTORAGE_CONFIG_DIR.",
  invalid_account:
    "Use letters, numbers, hyphens, or underscores for an account name.",
  account_missing:
    "This account directory is missing. Refresh the account list.",
  account_exists: "An account with that name already exists.",
  invalid_directory:
    "Choose an absolute directory path without control characters or surrounding whitespace.",
  unrepresentable_directory:
    "The CLI binding format cannot represent this path. Avoid equals signs and parent-directory segments.",
  directory_missing: "This directory no longer exists. Choose another folder.",
  symlink_storage:
    "Symlinked account storage is not supported for safety. Use the CLI to inspect this configuration.",
  storage_unreadable:
    "The account configuration could not be read. Check file permissions.",
  storage_write_failed:
    "The configuration could not be saved. Check file permissions.",
  cli_missing: "The bundled CLI is missing. Reinstall the desktop application.",
  terminal_failed: "Terminal could not be opened.",
  launcher_failed: "The terminal launch request could not be prepared.",
  terminal_macos_only: "Terminal launch is currently supported on macOS only.",
  backend_failed: "The backend operation failed. Try again.",
};

export function errorMessage(error: unknown): string {
  return typeof error === "string" && error in errors
    ? errors[error as ErrorCode]
    : "Unable to reach the desktop backend. Open the installed app, then try again.";
}

export function launchMessage(account: string, launch: Launch): string {
  return launch.state === "started"
    ? `Started in Terminal under ${account}. Claude’s output, including any sign-in prompt, is in that window.`
    : "Terminal has the request but has not run it yet. If macOS asked for permission, allow it and the launch continues.";
}
