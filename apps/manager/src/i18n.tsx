// Every piece of prose in the manager lives here (#166). `index.html` keeps a
// title and `lang` of its own only until `CopyProvider` replaces them.
//
// Every language supplies the same keys with the same shapes as English.
// Nothing checks that a value is translated, and nothing falls back from one
// language to another.
//
// Which ways of writing a table `tsc` catches mistakes in, and which slip, is
// in `copy.types.test.ts` as compiled cases, not here. Keep the
// `satisfies Record<…>` sub-tables as they are: a cast would accept a missing
// state.
//
// While English is the only language, the shape comparison in `copy.test.ts`
// compares English with itself and catches nothing; it starts to matter with
// the second one.
//
// Values that interpolate are functions, and their arguments are typed. A
// translation that ignores an argument, or drops the ReactNode it is handed
// for inline markup, still compiles, so check that in review.
import { createContext, useContext, useLayoutEffect, type ReactNode } from "react";
import type {
  EditorState,
  ErrorCode,
  IdentitySource,
  Launch,
  LockStatus,
  UpdateCheck,
  UsageSource,
} from "./contract";
import { isBackupFailure, type BackupErrorCode, type Reason, type Step, type Why } from "./backup/types";

const en = {
  locale: "en",

  productName: "Claude Accounts",
  productTagline: "PROJECT-AWARE WORKSPACE",
  eyebrow: "CLAUDE CODE · ACCOUNT MANAGER",
  dialogEyebrow: "ACCOUNT MANAGER",
  navHeading: "WORKSPACE",
  navAria: "Main navigation",
  breadcrumbRoot: "Workspace",
  pages: {
    accounts: {
      label: "Accounts",
      title: "Your accounts",
      lead: "A clear view of every identity. The right account for every project.",
    },
    projects: {
      label: "Projects",
      title: "Project bindings",
      lead: "Choose once. The account follows the directory and its children.",
    },
    desktop: {
      label: "Desktop",
      title: "Claude Desktop profiles",
      lead: "Separate Claude Desktop profiles, each signed in to its own account, open side by side.",
    },
    sessions: {
      label: "Sessions",
      title: "Session library",
      lead: "Discover conversations across accounts without opening their content.",
    },
    backup: {
      label: "Backup",
      title: "Backup and restore",
      lead: "Move accounts, sessions and projects' agent files to another machine, Mac to Windows included.",
    },
  },
  sidebarNoteTitle: "Directory-based by design",
  sidebarNoteBody:
    "Each terminal has its own account. Choosing a default never switches a running session.",
  appearance: "Appearance",
  themes: { system: "System", dark: "Dark", light: "Light" },
  liveRefresh: "Live refresh",
  liveRefreshAria: "Live refresh interval",
  manualOnly: "Manual only",
  everyMinutes: (minutes: number) => `Every ${minutes} min`,
  sidebarFooter: "LOCAL & PRIVATE",
  updateCheckAction: "Check for updates…",
  updateCheckTitle: "Check for a newer claude-acc?",
  updateCheckConfirm: "Check",
  updateCheckBody:
    "Ask GitHub for the latest claude-acc release and compare it with this app's CLI and the one installed in ~/.claude-switch/bin. Nothing is downloaded or installed.",
  updateCurrent: (latest: string) =>
    `No CLI here is known to be behind: the latest claude-acc release is ${latest}.`,
  updateUnknown: (latest: string) =>
    `The latest claude-acc release is ${latest}. This app's CLI did not report its version, so it cannot be compared.`,
  updateBundled: (latest: string, bundled: string) =>
    `claude-acc ${latest} is out; this app's CLI is ${bundled}. Rebuilding the app from that release brings the new one.`,
  updateInstalled: (latest: string, installed: string) =>
    `claude-acc ${latest} is out; the installed CLI is ${installed}. Run claude-acc update in Terminal to update it.`,
  version: (version: string) => `v${version}`,
  refreshTitle: "Refresh live identity and usage (⌘R)",
  refreshing: "Refreshing…",
  refresh: "Refresh status",
  addAccount: "Add account",
  footerPrivacy: "Runs on your Mac · no telemetry",
  footerMotto: "Accounts follow projects. Sessions stay yours.",

  // Shared controls
  cancel: "Cancel",
  done: "Done",
  working: "Working…",
  retry: "Retry",
  needsAttention: "Something needs attention",
  dismissNotice: "Dismiss notification",
  notKnown: "Not known",
  openClaude: "Open Claude",
  opening: "Opening…",

  // Accounts
  standardAccount: "Standard account",
  identityUnknown: "Identity not yet known",
  defaultBadge: "★ Default",
  planUnknown: "Plan unknown",
  identityDrift: "Identity drift",
  corruptPin: "Corrupt pin",
  identitySources: {
    live: "● Verified live",
    local: "○ Local identity",
    offline: "○ Offline identity",
    no_credentials: "○ No credentials",
    unknown: "○ Unverified",
  } satisfies Record<IdentitySource, string>,
  lockStates: {
    matched: "pin matches",
    drift: "pin differs from the signed-in identity",
    unpinned: "not pinned",
    unknown: "pinned, signed-in identity unknown",
    corrupt: "pin unreadable",
  } satisfies Record<LockStatus, string>,
  usageSources: {
    live: "Live from the API",
    cached: "Claude Code's own last reading",
    no_credentials: "No credentials",
    unavailable: "No reading",
  } satisfies Record<UsageSource, string>,
  identityTooltip: (identity: string, lock: string) =>
    `Identity: ${identity}; pin: ${lock}`,
  fiveHourWindow: "5-hour window",
  sevenDayWindow: "7-day window",
  actionsFor: (name: string) => `Actions for ${name}`,
  accountActions: "Account actions",
  detailsAction: "Details & diagnostics",
  loginAction: "Authenticate / re-login",
  makeDefaultAction: "Make default…",
  pinAction: "Pin identity…",
  repinAction: "Re-pin to signed-in identity…",
  cloneSettingsAction: "Copy settings from the standard account…",
  statuslineAction: "Install status line…",
  trashAction: "Move to Trash…",
  loginTitle: (name: string) => `Authenticate ${name}?`,
  loginConfirm: "Open login",
  loginBody:
    "Open the existing Claude login flow in Terminal. Check the browser identity before signing in. Return here and refresh after the login finishes.",
  pinTitle: (name: string) => `Pin ${name} to its identity?`,
  pinConfirm: "Pin",
  pinBody: (name: ReactNode) => (
    <>
      Pin {name} to the identity it is signed in as now. If it is later signed
      in as someone else, launching it is blocked until it is re-pinned.
    </>
  ),
  repinTitle: (name: string) => `Re-pin ${name}?`,
  repinConfirm: "Re-pin",
  repinBody: (name: ReactNode) => (
    <>
      {name} is signed in as someone other than its pin, or its pin cannot be
      read. Re-pinning accepts the identity signed in now and unblocks
      launching. Do it only if signing in as that identity was intended.
    </>
  ),
  cloneSettingsTitle: (name: string) => `Copy settings into ${name}?`,
  cloneSettingsConfirm: "Copy settings",
  cloneSettingsBody: (name: ReactNode) => (
    <>
      Copy settings.json, CLAUDE.md, agents, commands, output styles, skills and
      plugins from the standard account into {name}. A file or folder {name}
      already has is kept, and plugins are copied only if it has none
      installed. Sign-in, sessions and local settings are not copied.
    </>
  ),
  cloneSettingsDone: (name: string, items: string) => `Copied ${items} into ${name}.`,
  cloneSettingsNothing: (name: string) =>
    `Nothing to copy: ${name} already has these, or the standard account has none of them.`,
  statuslineTitle: (name: string) => `Install the status line for ${name}?`,
  statuslineConfirm: "Install",
  statuslineReplaceConfirm: "Replace status line",
  statuslineBody: (name: ReactNode) => (
    <>
      Add a statusLine to the settings.json of {name} that runs claude-acc
      statusline, showing which account a session is using. The rest of the
      file is kept.
    </>
  ),
  statuslineDone: (name: string) =>
    `Status line installed for ${name}. Restart Claude Code to see it.`,
  trashTitle: (name: string) => `Move ${name} to the Trash?`,
  trashConfirm: "Move to Trash",
  trashBody: (name: ReactNode) => (
    <>
      The account directory of {name}, with its settings and sessions, moves to
      the Trash and stays there until you empty it. Directories bound to it lose
      their binding, and if it is the default, the standard account becomes the
      default.
    </>
  ),
  defaultTitle: (name: string) => `Make ${name} the default?`,
  defaultConfirm: "Change default",
  defaultBody: (name: ReactNode) => (
    <>
      New terminals in directories without a binding will use {name}. Existing
      Claude processes keep their account.
    </>
  ),
  details: {
    identity: "Identity",
    verification: "Verification",
    subscription: "Subscription",
    organization: "Organization",
    configuration: "Configuration",
    usageSource: "Usage source",
    bundledCli: "Bundled CLI",
    installedCli: "Installed CLI",
    verificationValue: (identity: string, lock: string) => `${identity} · ${lock}`,
    directoryAvailable: "Directory available",
    directoryUnavailable: "Directory missing or unsafe",
    bundledValue: (version: string, available: boolean) =>
      `${version} · ${available ? "ready" : "missing"}`,
    checking: "Checking…",
    notInstalled: "Not installed at ~/.claude-switch/bin",
    note: "Local identity is a disk record. Refresh checks the profile API. Pin status compares the recorded identity with the CLI identity lock; it does not prove a live login.",
  },
  availableAccounts: "Available accounts",
  configuredDefault: "Configured default",
  lastChecked: "Last checked",
  allAccounts: "All accounts",
  searchAria: "Search accounts",
  searchPlaceholder: "Search accounts or email…",
  readingAccounts: "Reading account configuration…",
  accountsUnavailableTitle: "Account data unavailable",
  accountsUnavailableBody: "Retry to reach the manager's backend.",
  noAccountsTitle: "No configured accounts yet",
  noAccountsBody: "Add an account to open the existing Claude login flow in Terminal.",
  noMatchTitle: "No matching accounts",
  noMatchBody: "Try another name or email address.",
  accountsFootnote:
    "Usage is read from Claude’s API only when you refresh. Every reading shows its age, and becomes stale after ten minutes. No account rotation or background session sharing.",
  accountName: "Account name",
  accountNamePlaceholder: "e.g. work",
  continue: "Continue",
  accountNameInvalid:
    "Use letters, numbers, hyphens, or underscores. “default” is reserved.",
  addTitle: (name: string) => `Add ${name}?`,
  addConfirm: "Create & authenticate",
  addBody:
    "Create an isolated account using the CLI and open its login flow in Terminal. Conversations are not copied.",
  importInstead: "Import an existing folder instead",
  createInstead: "Create a new account instead",
  importSourceLabel: "Existing config folder",
  importSourcePlaceholder: "~/.claude-work",
  importTitle: (name: string) => `Import as ${name}?`,
  importBody: (source: ReactNode, name: ReactNode) => (
    <>
      {source} becomes the managed account {name}, with its settings, sessions
      and sign-in, without signing in again. On macOS, a Keychain sign-in found
      for the folder is copied to the new location.
    </>
  ),
  importMoveLabel:
    "Move the folder instead of copying it. The original location will no longer exist.",
  importCopyConfirm: "Import a copy",
  importMoveConfirm: "Move and import",
  importDone: (name: string) =>
    `Imported ${name}. Refresh status to check that it is still signed in; if it is not, use Authenticate from its menu.`,
  addSeedLabel:
    "Start with the standard account's settings.json, CLAUDE.md, agents, commands, output styles, skills and plugins",

  // Usage and freshness
  resetPassed: "Reset passed",
  meterUnavailable: "Unavailable",
  resetsAt: (when: string, passed: boolean) =>
    `${passed ? "Reset" : "Resets"} ${when}`,
  resetUnknown: "Reset time unavailable",
  ages: {
    justNow: "just now",
    minutes: (n: number) => `${n}m ago`,
    hours: (n: number) => `${n}h ago`,
    days: (n: number) => `${n}d ago`,
  },
  usage: {
    live: (age: string) => `Live · checked ${age}`,
    cached: (age: string) => `Cached by Claude Code · ${age}`,
    stale: (age: string) => `Stale · ${age} · refresh to update`,
    noCredentials: "No credentials available",
    unavailable: "No usage reading · refresh to check",
  },

  // Projects
  choosePrompt: "Choose a project",
  chooseLead: "Inspect the account resolved by the CLI’s directory rules.",
  directoryLabel: "Project directory",
  directoryPlaceholder: "/Users/you/Projects/my-project",
  browse: "Browse…",
  inspect: "Inspect",
  effectiveAccount: "EFFECTIVE ACCOUNT",
  inheritedFrom: (directory: string) => `From binding: ${directory}`,
  fromDefault: "From configured default (no parent-directory binding)",
  bindingAccountAria: "Account for selected project",
  saveBinding: "Save binding",
  directoryBindings: "Directory bindings",
  reloadBindings: "Reload bindings",
  noBindingsTitle: "Give your projects an account",
  noBindingsBody: "Choose a folder above to create your first binding.",
  bindingIncludesChildren: "Includes child directories unless overridden",
  bindingDirectoryMissing: "Directory missing — binding is still stored",
  remove: "Remove",
  projectsFootnote:
    "The closest parent binding wins. A binding to “default” explicitly selects the standard ~/.claude account.",
  saveTitle: "Save project binding?",
  saveBody: (account: ReactNode, directory: ReactNode) => (
    <>
      Use {account} for {directory} and its child directories, except where a
      closer binding exists.
    </>
  ),
  removeTitle: "Remove directory binding?",
  removeConfirm: "Remove binding",
  removeBody: (directory: ReactNode) => (
    <>
      Remove the binding for {directory}. Future resolution will fall back to a
      parent binding or the configured default.
    </>
  ),

  vscodeTitle: "VS Code integration",
  vscodeNone: "No VS Code, VS Code Insiders, VSCodium or Cursor found on this Mac.",
  editorStates: {
    ours: "on: the extension runs the agent under each workspace folder's account",
    unset: "off: the extension's own UI ignores the account",
    foreign: "set to another tool's launcher",
    unreadable: "settings.json cannot be read as JSON, so it is left alone",
    linked: "settings.json is a link, so it is left alone; edit it by hand",
  } satisfies Record<EditorState, string>,
  vscodeOtherProfiles: "(profiles other than the default read their own settings)",
  vscodeInstallAction: "Set up…",
  vscodeUninstallAction: "Remove…",
  vscodeInstallTitle: "Set up the VS Code integration?",
  vscodeInstallConfirm: "Set up",
  vscodeInstallBody:
    "Point claudeCode.claudeProcessWrapper in each editor's settings.json at a launcher in ~/.claude-switch/bin, so the Claude Code extension runs the agent under the workspace folder's account, as a terminal does. The rest of the extension, such as its session list, MCP servers and plugins, still reads ~/.claude. A launcher set by another tool, an unreadable settings.json and a linked one are left alone. With a process wrapper set, the extension resolves the permission mode itself instead of deferring to the CLI and stops checking for its own updates; that is its behaviour, not ours. Remove takes our launcher out again; one it replaced is not put back.",
  vscodeReplaceForeign: "Also replace the launcher another tool set",
  vscodeUninstallTitle: "Remove the VS Code integration?",
  vscodeUninstallConfirm: "Remove",
  vscodeUninstallBody:
    "Take claudeCode.claudeProcessWrapper out of each editor that points at our launcher. A launcher set by another tool, and a settings.json that is a link, are left in place.",
  vscodeRestart: "Restart the editor for it to take effect.",

  // Desktop
  desktopBanner:
    "Each profile is its own Claude Desktop data folder under ~/.claude-switch/desktop, as claude-acc desktop makes them. Signing in needs every other Claude Desktop window closed; after that, profiles open side by side.",
  desktopReading: "Reading Desktop profiles…",
  desktopNoneTitle: "No Desktop profiles yet",
  desktopNoneBody: "Add one above to open Claude Desktop on a profile of its own.",
  desktopAppProfile:
    "Claude Desktop's own profile is on this Mac too. It is the one the app opens normally, and it is not managed here.",
  desktopSignedIn: "Signed in",
  desktopSignedOut: "Not signed in",
  desktopOpen: "Open",
  desktopOpenAnyway: "Open anyway",
  desktopNewProfile: "New profile name",
  desktopAdd: "Add profile",
  desktopSeedLabel: "Start with the MCP servers and preferences of Claude Desktop's own profile",
  desktopAddTitle: (name: string) => `Add the Desktop profile ${name}?`,
  desktopAddBody:
    "Create the profile and open Claude Desktop on it, signed out: sign in there with the account it is for. Quit every other Claude Desktop window first, because signing in finishes through a claude:// link that the system hands to one instance only. The profile is fully isolated, so the app downloads its whole runtime into it again, about 10.5 GB.",
  desktopOpenTitle: (name: string) => `Open Claude Desktop on ${name}?`,
  desktopOpenBody: (name: ReactNode) => (
    <>
      Open Claude Desktop on {name}. If it has not signed in yet, quit every other
      Claude Desktop window first, so the sign-in reaches this one.
    </>
  ),
  desktopAdded: (name: string) => `Added ${name} and opened Claude Desktop on it. Sign in there.`,
  desktopOpened: (name: string) => `Opened Claude Desktop on ${name}.`,
  desktopUsageAction: "Usage…",
  desktopUsageUnavailable: "The usage API did not answer. Use Usage… again to retry.",
  desktopUsageTitle: (name: string) => `Read the usage of ${name}?`,
  desktopUsageConfirm: "Read usage",
  desktopUsageBody:
    "Reading a profile's account and usage means decrypting its token. On macOS the key is in the Claude Safe Storage keychain entry, so macOS will ask for your login keychain password; declining only costs this reading. On Windows the key is unwrapped for your user without asking. The profile and usage APIs are then asked, and the profile's account is remembered for the list.",
  desktopCopyAction: "Copy into…",
  desktopCopyTitle: (name: string) => `Copy into ${name}?`,
  desktopCopyWhat: "What",
  desktopCopyConfig: "MCP servers and preferences",
  desktopCopyRuntime: "Downloaded runtime",
  desktopCopyFrom: "From",
  desktopAppProfileName: "Claude Desktop's own profile",
  desktopCopyNoSource: "There is no other profile to copy from.",
  desktopCopyConfigBody:
    "Copies claude_desktop_config.json, the MCP server definitions and preferences. Servers that sign in separately will ask for that again in this profile.",
  desktopCopyRuntimeBody:
    "Clones the downloaded runtime, about 10.5 GB, so this profile need not download its own. On APFS the clone shares the original's disk blocks until one of them changes; the notice says what the disk actually lost. Whether the app accepts a runtime put there for it is untested: if the profile misbehaves, delete its vm_bundles, claude-code and claude-code-vm folders and it fetches its own.",
  desktopCopyConfirm: "Copy",
  desktopCopyReplace: "Replace",
  desktopConfigCopied: (name: string, from: string) =>
    `Copied the MCP servers and preferences of ${from} into ${name}.`,
  desktopRuntimeCloned: (name: string, from: string, size: string, lost: string | null) =>
    `Cloned ${size} of runtime from ${from} into ${name}. ${
      lost == null ? "The disk space used could not be measured." : `The disk lost ${lost}.`
    } If the profile misbehaves, delete its vm_bundles, claude-code and claude-code-vm folders.`,
  sizeGigabytes: (gigabytes: string) => `${gigabytes} GB`,
  desktopTrashTitle: (name: string) => `Move the Desktop profile ${name} to the Trash?`,
  desktopTrashBody: (name: ReactNode) => (
    <>
      The data folder of {name}, with its sign-in, settings and downloaded runtime,
      moves to the Trash and stays there until you empty it.
    </>
  ),

  // Sessions
  sessionsBanner:
    "This browser reads session metadata only, never a transcript's content. Copying a session to another account leaves the original where it is.",
  discoveringSessions: "Discovering session metadata…",
  noSessionsTitle: "No sessions found",
  noSessionsBody: "Start Claude Code under an account to create a conversation.",
  showMore: (remaining: number) => `Show more (${remaining} remaining)`,
  sessionsFootnote:
    "Project names are the CLI’s stored directory slugs, not reversible filesystem paths. Resuming a copied session happens in Terminal.",
  modifiedUnknown: "Modified time unknown",
  sessionSize: (kilobytes: string) => `${kilobytes} KB`,
  copiesElsewhere: "Copies in multiple accounts",
  resumeHookTitle: "Resume check in the claude wrapper (installed by claude-acc install)",
  resumeHookOn:
    "On: claude --resume <id> offers to copy a session that another account holds.",
  resumeHookOff:
    "Off: claude --resume <id> goes straight through. claude-acc run <account> --resume <id> still checks.",
  resumeHookTurnOff: "Turn off…",
  resumeHookTurnOn: "Turn on…",
  resumeHookOffTitle: "Turn off the resume check?",
  resumeHookOnTitle: "Turn on the resume check?",
  resumeHookOffConfirm: "Turn off",
  resumeHookOnConfirm: "Turn on",
  resumeHookOffBody:
    "claude --resume <id> will no longer look for the session in your other accounts. claude-acc run <account> --resume <id> keeps checking. It applies to the claude wrapper that claude-acc install puts on PATH; without it, claude runs Claude Code directly and nothing is checked. A terminal with CLAUDE_ACC_NO_RESUME_HOOK set to a value other than empty or 0 skips the check whatever is stored here.",
  resumeHookOnBody:
    "claude --resume <id> will look for the session in your other accounts first and offer to copy it, asking before anything is copied. It applies to the claude wrapper that claude-acc install puts on PATH; without it, claude runs Claude Code directly and nothing is checked. A terminal with CLAUDE_ACC_NO_RESUME_HOOK set to a value other than empty or 0 skips the check whatever is stored here.",
  copySessionAction: "Copy to…",
  copySessionAria: (id: string, account: string) =>
    `Copy session ${id} from ${account} to another account`,
  copySessionTitle: "Copy this session to another account?",
  copySessionBody: (account: ReactNode, id: ReactNode) => (
    <>
      The transcript of {id}, with its subagent transcripts, is copied from{" "}
      {account}. The original stays where it is.
    </>
  ),
  copySessionDestination: "Destination account",
  copySessionReplaceWarning: (account: ReactNode) => (
    <>{account} already has a copy of this session. Copying replaces it.</>
  ),
  copySessionCost:
    "The prompt cache is per account, so the first message after resuming under another account re-sends the whole transcript: slower and more expensive than a normal turn.",
  copySessionConfirm: "Copy session",
  copySessionReplaceConfirm: "Replace copy",
  copySessionDone: (account: string, id: string) =>
    `Copied to ${account}. To continue it, run claude-acc run ${account} --resume ${id} in Terminal.`,

  // Backup
  backupCreateTitle: "Create an encrypted backup",
  backupCreateLead:
    "Accounts, projects' agent files, or both, in one file locked with a passphrase. Credentials never go in.",
  backupAccountsLegend: "Accounts to back up whole",
  backupProjectsLegend: "Projects: their CLAUDE.md, AGENTS.md, .mcp.json, .claude and .agents, never their source code",
  backupSelectAll: "Select all",
  backupSelectNone: "Select none",
  backupAddProject: "Add a project folder…",
  backupRemoveProjectAria: (path: string) => `Remove ${path} from the backup`,
  backupModes: {
    accounts: "Accounts only: each chosen account whole, with its bindings and the default account.",
    projects:
      "Projects only: their agent files, and their sessions and memory from every account. No account is taken whole.",
    both: "Both: the chosen accounts whole, and the projects' agent files and sessions.",
    nothing: "Choose accounts, projects, or both.",
  } satisfies Record<"accounts" | "projects" | "both" | "nothing", string>,
  backupPreview: "Preview",
  backupPreviewFirst: "Preview first: the restore applies exactly what was previewed.",
  backupCreateAction: "Create backup…",
  backupWillHold: "THIS BACKUP WILL HOLD",
  backupFilesAndSize: (files: number, size: string) => `${files} files, ${size}`,
  backupWholeAccounts: (names: string) => `Whole accounts: ${names}`,
  backupProjectLine: (path: string) => `Project: ${path}`,
  backupProjectsOnlyNote:
    "From each account, only these projects' sessions and memory are taken.",
  backupEmptyProject: (path: string) =>
    `No agent files found in ${path}: it is not a folder, or holds nothing to take.`,
  backupLeftOut: (n: number) => `${n} entries left out (credentials, caches, plugins…)`,
  backupReasons: {
    credential: "credential",
    runtime: "runtime",
    reinstall: "reinstall",
    special: "not a file",
    checkout: "checkout",
  } satisfies Record<Reason, string>,
  backupCreateBody: (files: number, size: string) =>
    `${files} files, ${size}, are encrypted into one file. A file that changes while it is read fails the backup, so close Claude Code sessions first.`,
  backupFileLabel: "Backup file",
  backupFilePlaceholder: "Where to save the .ccbackup file",
  backupPassphrase: "Passphrase",
  backupPassphraseAgain: "Passphrase again",
  backupPassphraseMismatch: "The two passphrases differ.",
  backupPassphraseWarning:
    "There is no way to recover a forgotten passphrase: without it the backup cannot be opened.",
  backupCreateConfirm: "Encrypt and save",
  backupWritten: "BACKUP WRITTEN",
  backupWrittenBody: (files: number) =>
    `${files} files, every one with its checksum. Copy this file to the other machine.`,
  backupRedacted: (n: number) =>
    `${n} secret fields removed; set them again on the new machine`,
  backupRestoreTitle: "Restore a backup",
  backupRestoreLead:
    "Open a backup to see what it holds, choose what goes where, preview every file, then restore. Nothing is overwritten unless you ask.",
  backupOpenPlaceholder: "A .ccbackup file",
  backupOpenAction: "Open",
  backupOpening: "Decrypting and checking…",
  backupVerified: (files: number) => `CHECKED: ${files} FILES, ALL CHECKSUMS MATCH`,
  backupOrigin: (when: string, platform: string, home: string, version: string) =>
    `Made ${when} on ${platform} (home ${home}) by claude-acc ${version}.`,
  backupOpenedAccount: (files: number, projectsOnly: boolean) =>
    `${files} files${projectsOnly ? ", project sessions only" : ""}`,
  backupProjectBadge: "project",
  backupBindingBadge: "binding",
  backupFileCount: (files: number) => `${files} files`,
  backupCloseAction: "Close this backup",
  backupRestoreModeLegend: "What to restore",
  backupRestoreModes: {
    everything: "Everything chosen below",
    accounts: "Accounts only",
    projects: "Projects only (same project, with its sessions)",
    reuse: "Reuse skills and agents in another project",
  } satisfies Record<"everything" | "accounts" | "projects" | "reuse", string>,
  backupRestoreModeHints: {
    everything:
      "The chosen accounts whole, and the chosen projects with their sessions and memory.",
    accounts: "The chosen accounts whole. No project files.",
    projects:
      "The chosen projects' agent files, and their sessions and memory into the accounts of the same name, which may already exist.",
    reuse:
      "Only skills, agents, rules and commands from the project's .claude and .agents, into another folder. Never its settings, state, memory or sessions.",
  } satisfies Record<"everything" | "accounts" | "projects" | "reuse", string>,
  backupMapsLegend: "Path mapping",
  backupMapsHint:
    "Where the source machine's folders are on this one, e.g. /Users/me/work → F:\\work. Session folders, project paths in .claude.json and bindings follow it. Transcripts are never rewritten.",
  backupMapFrom: "On the source machine",
  backupMapTo: "On this machine",
  backupMapToPlaceholder: "Folder on this machine",
  backupAddMap: "Add a mapping",
  backupRemoveMapAria: "Remove this mapping",
  backupRestoreAccountsLegend: "Accounts",
  backupRestoreAccountsHint:
    "Accounts are never merged: if one of that name exists here, restore it under another name.",
  backupRestoreAs: (name: string) => `Restore ${name} as`,
  backupRestoreProjectsLegend: "Projects",
  backupDestinationHint: "Leave a destination empty to use the path mapping.",
  backupReuseDestinationHint: "Each chosen project needs the folder of the project to copy into.",
  backupDestinationFor: (path: string) => `Destination for ${path}`,
  backupFromMapping: "From the path mapping",
  backupReuseDestinationPlaceholder: "The other project's folder",
  backupSelectLegend: "Only these (optional)",
  backupSelectPlaceholder: "skills/deploy\nagents/review.md",
  backupSelectHint: "One per line, e.g. skills/deploy or agents/review.md. Empty takes all four kinds.",
  backupReplace: "Replace files that differ, keeping each original",
  backupRestoreAction: "Restore…",
  backupPlanTitle: "PREVIEW",
  backupPlanStale: "PREVIEW OUT OF DATE: PREVIEW AGAIN",
  backupPlanCounts: (create: number, same: number, conflict: number, bad: number) =>
    `${create} to create · ${same} already in place · ${conflict} conflicts · ${bad} cannot be written here`,
  backupAccountMove: (to: string, isNew: boolean, whole: boolean) =>
    `→ ${to}${isNew ? " (new)" : ""}${whole ? "" : ", project sessions only"}`,
  backupProjectLeftOut: (path: string) =>
    `${path} is not restored: no mapping or destination covers it.`,
  backupSteps: {
    create: "new",
    same: "already in place",
    conflict: "differs here",
    incompatible: "cannot be written here",
  } satisfies Record<Step, string>,
  backupLoginNeeded: (names: string) =>
    `Sign in again here, since credentials never travel: ${names}.`,
  backupFilterAria: "Show files by what the restore does",
  backupAllFiles: (n: number) => `All (${n})`,
  backupStepFilter: (step: Step, n: number) =>
    `${({ create: "New", same: "In place", conflict: "Conflicts", incompatible: "Cannot write" } satisfies Record<Step, string>)[step]} (${n})`,
  backupWhy: {
    symlink_on_windows: "a symlink; Windows needs Developer Mode for those",
    link_leaves_folder: "a link pointing outside its folder",
    name_character: "a character Windows does not allow in names",
    name_trailing: "a name ending in a dot or space",
    name_reserved: "a device name Windows reserves",
    same_name_other_case: "differs from another file only by case",
    folder_in_the_way: "a folder of that name is already there",
    link_in_the_way: "a symlink is already there",
    not_a_link_there: "something else is already where the link goes",
  } satisfies Record<Why, string>,
  backupWhyCase: (other: string) => `differs from ${other} only by case`,
  backupNoteRenamed: (account: string, from: string, to: string) =>
    `Sessions of ${account}: projects/${from} → projects/${to}, for the new path.`,
  backupNoteUnmapped: (account: string, name: string) =>
    `Sessions of ${account} in projects/${name} keep their name: no mapping covers their folder, so Claude Code here will not offer them.`,
  backupNoteJsonMapped: (account: string, keys: number) =>
    `${account}: ${keys} project paths in .claude.json moved to the new paths.`,
  backupNoteReview: (path: string, field: string) =>
    `Check by hand, written for the old machine: ${path} → ${field}`,
  backupNoteLinkUnmapped: (dir: string) => `Binding for ${dir} not restored: no mapping covers it.`,
  backupNoteReinstall: (path: string) => `Install again here: ${path}`,
  backupReplaceOn: "Files that differ are replaced; each original is kept first.",
  backupReplaceOff: "Files that differ are left as they are.",
  backupRestoreConfirmTitle: "Restore this backup?",
  backupRestoreConfirmBody: (create: number, replace: number, keep: number) =>
    `${create} files are created${replace ? `, ${replace} replaced (originals kept)` : ""}${keep ? `, ${keep} left as they are` : ""}. Every step is logged; if anything fails, what was done is undone.`,
  backupRestoreConfirm: "Restore",
  backupRestoredNotice: (written: number, replaced: number) =>
    `Restored: ${written} files written, ${replaced} replaced.`,
  backupRestored: "RESTORED",
  backupAppliedCounts: (written: number, replaced: number, skipped: number, switches: number) =>
    `${written} written · ${replaced} replaced · ${skipped} skipped · ${switches} binding/default changes`,
  backupOriginalsKept: (path: string) => `Replaced originals are in ${path}`,
  backupResumeNote:
    "The files are in place. Whether a session resumes is up to Claude Code here: check with claude --resume inside the project folder.",
  backupPendingTitle: "An earlier restore did not finish",
  backupPendingBody: (path: string) =>
    `Its log is in ${path}. Undo it before restoring again.`,
  backupRollbackAction: "Undo it…",
  backupRollbackTitle: "Undo the unfinished restore?",
  backupRollbackBody:
    "Files it created are removed and files it replaced are put back, from its own log. Folders it made are removed only if empty.",
  backupRollbackConfirm: "Undo",
  backupRolledBack: "The unfinished restore was undone.",
  sizeMegabytes: (megabytes: string) => `${megabytes} MB`,
  backupErrors: {
    backup_io: (s: string | null) => `A file could not be read or written${s ? `: ${s}` : ""}.`,
    backup_source_changed: (s: string | null) =>
      `${s ?? "A file"} changed while being backed up. Close Claude Code sessions and try again; nothing was written.`,
    backup_nothing_selected: () => "Nothing is selected.",
    backup_unknown_account: (s: string | null) => `There is no account named ${s ?? "that"}.`,
    backup_wrong_passphrase: () => "Wrong passphrase.",
    backup_corrupt: () =>
      "This is not a Claude Accounts backup, or it is damaged or cut short. Nothing was restored.",
    backup_unsupported_format: (s: string | null) =>
      `This backup is format ${s ?? "?"}, which this version cannot read. Update Claude Accounts.`,
    backup_unsafe_path: (s: string | null) =>
      `Refused: the backup holds an entry outside its own folder (${s ?? "?"}).`,
    backup_checksum: (s: string | null) =>
      `Refused: ${s ?? "a file"} does not match the backup's checksum list.`,
    backup_pending: () => "An earlier restore did not finish. Undo it first, at the top of this page.",
    backup_account_exists: (s: string | null) =>
      `An account named ${s ?? "that"} already exists here; accounts are never merged. Restore it under another name.`,
    backup_unmapped_project: (s: string | null) =>
      `No destination for ${s ?? "a project"}: give it one, or add a mapping that covers it.`,
    backup_duplicate_target: (s: string | null) =>
      `Two accounts would go into ${s ?? "one"}; accounts are never merged.`,
    backup_bad_map: (s: string | null) => `A mapping needs both folders: ${s ?? ""}`,
    backup_empty_passphrase: () => "Enter a passphrase.",
    backup_passphrase_mismatch: () => "The two passphrases differ.",
    backup_file_exists: (s: string | null) =>
      `${s ?? "That file"} already exists. Choose a new file name.`,
    backup_not_open: () => "Open the backup again.",
    backup_failed: () => "The backup operation failed. Try again.",
  } satisfies Record<BackupErrorCode, (subject: string | null) => string>,

  // Backend results
  errors: {
    desktop_not_signed_in: "This profile is not signed in, so there is no usage to read.",
    keychain_denied:
      "There was no access to the keychain, so the account behind this profile stays unknown.",
    desktop_token_unreadable:
      "The profile's credentials could not be read; Claude Desktop may have changed their format.",
    desktop_token_expired:
      "The profile's credentials have expired. Open the profile in Claude Desktop to refresh them, then try again.",
    desktop_nothing_to_copy: "The profile chosen to copy from has nothing of that kind to copy.",
    desktop_already_has:
      "This profile already has its own. Confirm again to replace it with the copy.",
    desktop_runtime_would_copy:
      "The two profiles are on different volumes, so this would be a full copy of the runtime, not a clone. Nothing was copied.",
    desktop_running:
      "Another Claude Desktop is open. A sign-in needs every other window closed, and a profile that is open cannot be moved to the Trash. Quit it and try again.",
    desktop_app_missing:
      "Claude Desktop was not found in /Applications or ~/Applications.",
    desktop_launch_failed: "Claude Desktop could not be opened.",
    update_check_failed:
      "GitHub could not be asked for the latest release. Check the connection and try again.",
    unsupported_platform: "This is not available on this platform yet.",
    cli_not_installed:
      "This runs claude-acc from ~/.claude-switch/bin, which is not installed. Install the CLI with claude-acc install first.",
    statusline_exists:
      "This account already has a status line of its own. Confirm again to replace it; the rest of settings.json is kept.",
    import_source_refused:
      "This folder cannot be imported: it is already a managed account, is a link or keeps its sign-in files as links, holds Claude Accounts' own storage, or, for a move, holds the standard ~/.claude account.",
    session_missing:
      "This session is no longer in that account, so nothing was copied. The list has been reloaded.",
    session_exists:
      "The destination already has a copy of this session, so nothing was copied. The list has been reloaded; confirm again to replace it.",
    not_signed_in:
      "This account has never signed in, so there is no identity to pin. Sign in first, then pin it.",
    trash_unavailable:
      "The account could not be moved to the Trash, so nothing was removed. `claude-acc remove --purge` in Terminal deletes it outright.",
    identity_drift:
      "The account identity differs from its pin, or the pin is corrupt. If signing in as this identity was intended, re-pin it from the account menu; otherwise sign in again as the pinned one.",
    shared_credential_store:
      "A shared credential-store override is set. Live refresh is disabled because it could report one account for every directory. Launch the manager without CLAUDE_SECURESTORAGE_CONFIG_DIR.",
    invalid_account:
      "Use letters, numbers, hyphens, or underscores for an account name.",
    account_missing: "This account or profile no longer exists. Refresh the list.",
    account_exists: "An account or profile with that name already exists.",
    invalid_directory:
      "Choose an absolute directory path without control characters or surrounding whitespace.",
    unrepresentable_directory:
      "This path cannot be used as written. Remove parent-directory (..) segments; a folder binding also cannot contain an equals sign.",
    directory_missing: "This directory no longer exists. Choose another folder.",
    symlink_storage:
      "Symlinked account storage is not supported for safety. Use the CLI to inspect this configuration.",
    storage_unreadable:
      "The account configuration could not be read. Check file permissions.",
    storage_write_failed:
      "The change could not be saved, or was saved only in part. Check file permissions and free disk space, then try again.",
    cli_missing: "The bundled CLI is missing. Reinstall Claude Accounts.",
    terminal_failed: "Terminal could not be opened.",
    launcher_failed: "The terminal launch request could not be prepared.",
    terminal_macos_only: "Terminal launch is currently supported on macOS only.",
    backend_failed: "The backend operation failed. Try again.",
  } satisfies Record<ErrorCode, string>,
  unreachable:
    "Unable to reach the manager's backend. Open the installed app, then try again.",
  unrecognised:
    "The backend answered with an error this window does not recognise. Try again; if it repeats, reinstall Claude Accounts.",
  launchStarted: (account: string) =>
    `Started in Terminal under ${account}. Claude’s output, including any sign-in prompt, is in that window.`,
  launchPending:
    "Terminal has the request but has not run it yet. If macOS asked for permission, allow it and the launch continues.",
};

export type Messages = typeof en;

/**
 * The languages with a complete table. `ru` joins when its table exists
 * (#166); adding it here makes `tsc` require every key above.
 */
export const LANGS = ["en"] as const;
export type Lang = (typeof LANGS)[number];

export const messages: Record<Lang, Messages> = { en };

// No default value: a component rendered outside the provider is a bug to
// surface, not a reason to quietly pick a language.
const Copy = createContext<Messages | null>(null);

export function CopyProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  const t = messages[lang];
  // `index.html` cannot know the language; the document's `lang` decides how
  // a screen reader pronounces the page, so it follows the table, as does the
  // window title.
  useLayoutEffect(() => {
    document.documentElement.lang = t.locale;
    document.title = t.productName;
  }, [t]);
  return <Copy.Provider value={t}>{children}</Copy.Provider>;
}

/** The active language's table. */
export function useCopy(): Messages {
  const t = useContext(Copy);
  if (!t) throw new Error("useCopy outside CopyProvider");
  return t;
}

/**
 * Copy for a failed request. A backend code gets its own message. Any other
 * string means the backend did answer, with something this window does not
 * recognise, so it must not be told the backend is unreachable. Only a
 * non-string, a request that never got an answer, is that. Tauri's own
 * rejections are strings of the second kind, read from tauri 2.12.4: its
 * errors reach the window through `From<crate::Error> for InvokeError`
 * (`ipc/mod.rs`) as `Value::String(error.to_string())`, bad arguments
 * included, and an unknown command is rejected with
 * `format!("Command {command} not found")` (`webview/mod.rs`).
 */
export function describeError(t: Messages, error: unknown): string {
  // Backup commands answer with `{ code, subject }`, naming what failed.
  if (isBackupFailure(error)) {
    return Object.hasOwn(t.backupErrors, error.code)
      ? t.backupErrors[error.code](error.subject)
      : t.unrecognised;
  }
  if (typeof error !== "string") return t.unreachable;
  // `hasOwn`, not `in`: `in` also admits "toString", "constructor" and the
  // rest of the prototype chain, which are functions, not copy.
  return Object.hasOwn(t.errors, error) ? t.errors[error as ErrorCode] : t.unrecognised;
}

/** A size in kilobytes, with the table's decimal separator. */
export function kilobytes(t: Messages, bytes: number): string {
  return t.sessionSize(
    (bytes / 1024).toLocaleString(t.locale, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }),
  );
}

/** A size in megabytes, with the table's decimal separator. */
export function megabytes(t: Messages, bytes: number): string {
  return t.sizeMegabytes(
    (bytes / 1024 / 1024).toLocaleString(t.locale, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }),
  );
}

/** Items joined the table's way: "a, b, and c" in English. */
export function list(t: Messages, items: string[]): string {
  return new Intl.ListFormat(t.locale, { type: "conjunction" }).format(items);
}

/** One notice for an update check, naming each CLI that is behind. */
export function describeUpdate(t: Messages, check: UpdateCheck): string {
  const behind = [
    check.bundled_outdated && check.bundled && t.updateBundled(check.latest, check.bundled),
    check.installed_outdated &&
      check.installed &&
      t.updateInstalled(check.latest, check.installed),
  ].filter((line): line is string => typeof line === "string");
  if (behind.length) return behind.join(" ");
  return check.bundled == null ? t.updateUnknown(check.latest) : t.updateCurrent(check.latest);
}

/** A size in gigabytes (1024³ bytes), with the table's decimal separator. */
export function gigabytes(t: Messages, bytes: number): string {
  return t.sizeGigabytes(
    (bytes / 1024 ** 3).toLocaleString(t.locale, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    }),
  );
}

export function describeLaunch(t: Messages, account: string, launch: Launch): string {
  return launch.state === "started" ? t.launchStarted(account) : t.launchPending;
}
