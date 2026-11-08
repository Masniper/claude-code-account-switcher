// The only module that talks to the Tauri runtime. Every request and every
// event subscription goes through here, so changing how the backend is
// reached, or how its updates arrive, touches this file and nothing else.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Bindings,
  DesktopAction,
  DesktopProfiles,
  DesktopUsage,
  Editor,
  Installation,
  RuntimeCloned,
  Launch,
  LaunchAction,
  Session,
  Snapshot,
  UpdateCheck,
  WatchStatus,
} from "./contract";
import type { Applied, Created, Opened, Plan, RestoreChoice, Survey } from "./backup/types";

export const api = {
  snapshot: (live: boolean) => invoke<Snapshot>("snapshot", { live }),
  setDefault: (account: string) => invoke<Snapshot>("set_default", { account }),
  removeAccount: (account: string) =>
    invoke<Snapshot>("remove_account", { account }),
  importAccount: (account: string, source: string, moveInto: boolean) =>
    invoke<Snapshot>("import_account", { account, source, moveInto }),
  pinAccount: (account: string, force: boolean) =>
    invoke<Snapshot>("pin_account", { account, force }),
  cloneSettings: (account: string) => invoke<string[]>("clone_settings", { account }),
  installStatusline: (account: string, replace: boolean) =>
    invoke<void>("install_statusline", { account, replace }),
  bindings: (directory: string | null) =>
    invoke<Bindings>("list_bindings", { directory }),
  bind: (directory: string, account: string | null) =>
    invoke<void>("set_binding", { directory, account }),
  sessions: () => invoke<Session[]>("list_sessions"),
  resumeHook: () => invoke<boolean>("resume_hook"),
  setResumeHook: (on: boolean) => invoke<boolean>("set_resume_hook", { on }),
  copySession: (from: string, project: string, id: string, to: string, replace: boolean) =>
    invoke<Session[]>("copy_session", { from, project, id, to, replace }),
  vscodeEditors: () => invoke<Editor[]>("vscode_editors"),
  vscodeInstall: (force: boolean) => invoke<Editor[]>("vscode_install", { force }),
  vscodeUninstall: () => invoke<Editor[]>("vscode_uninstall"),
  terminal: (account: string, directory: string | null, action: LaunchAction) =>
    invoke<Launch>("open_terminal", { account, directory, action }),
  desktopProfiles: () => invoke<DesktopProfiles>("desktop_profiles"),
  desktopOpen: (account: string, action: DesktopAction, force: boolean) =>
    invoke<DesktopProfiles>("desktop_open", { account, action, force }),
  desktopCloneConfig: (account: string, from: string | null, force: boolean) =>
    invoke<void>("desktop_clone_config", { account, from, force }),
  desktopCloneRuntime: (account: string, from: string | null, force: boolean) =>
    invoke<RuntimeCloned>("desktop_clone_runtime", { account, from, force }),
  desktopUsage: (account: string) => invoke<DesktopUsage>("desktop_usage", { account }),
  desktopRemove: (account: string) => invoke<DesktopProfiles>("desktop_remove", { account }),
  installation: () => invoke<Installation>("installation"),
  checkUpdate: () => invoke<UpdateCheck>("check_update"),
  chooseDirectory: () => invoke<string | null>("choose_directory"),
  vpnExit: () => invoke<{ city: string; country: string; zone: string }>("vpn_exit"),
  canRestoreZone: () => invoke<boolean>("can_restore_zone"),
  changeVpnZone: (zone: string | null) => invoke<boolean>("change_vpn_zone", { zone }),
  watchStatus: () => invoke<WatchStatus>("watch_status"),
  setWatch: (on: boolean) => invoke<WatchStatus>("set_watch", { on }),
  backupSurvey: (accounts: string[], allAccounts: boolean, projects: string[]) =>
    invoke<Survey>("backup_survey", { accounts, allAccounts, projects }),
  backupCreate: (
    file: string,
    accounts: string[],
    allAccounts: boolean,
    projects: string[],
    passphrase: string,
  ) =>
    invoke<Created>("backup_create", { file, accounts, allAccounts, projects, passphrase }),
  backupOpen: (file: string, passphrase: string) =>
    invoke<Opened>("backup_open", { file, passphrase }),
  backupPreview: (choice: RestoreChoice) => invoke<Plan>("backup_preview", { choice }),
  backupRestore: (choice: RestoreChoice) => invoke<Applied>("backup_restore", { choice }),
  backupClose: () => invoke<void>("backup_close"),
  backupPending: () => invoke<string | null>("backup_pending"),
  backupRollback: () => invoke<string | null>("backup_rollback"),
  chooseBackupSave: () => invoke<string | null>("choose_backup_save"),
  chooseBackupFile: () => invoke<string | null>("choose_backup_file"),
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
