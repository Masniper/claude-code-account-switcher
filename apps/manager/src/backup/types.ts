// What the backup commands send back, as `src-tauri/src/backup.rs` builds
// it. Every field is a path, a count, an account name or a field name.

export type Reason = "credential" | "runtime" | "reinstall" | "special" | "checkout";

export type Left = { path: string; reason: Reason };
export type Removed = { path: string; field: string };

export type Survey = {
  accounts: string[];
  projects: string[];
  projects_only: boolean;
  files: number;
  bytes: number;
  empty_projects: string[];
  excluded: Left[];
};

export type Created = { file: string; files: number; redactions: Removed[] };

export type Opened = {
  file: string;
  files: number;
  created_at: number;
  source_platform: string;
  source_home: string;
  tool_version: string;
  accounts: { label: string; source_dir: string; files: number; projects_only: boolean }[];
  projects: { source_root: string; files: number }[];
  bindings: { dir: string; account: string }[];
  default_account: string | null;
  suggested_from: string;
  redactions: Removed[];
  excluded: Left[];
};

/** A restore as a person describes it; the library's `RestoreChoice`. */
export type RestoreChoice = {
  maps: string[];
  accounts: string[];
  all_accounts: boolean;
  projects: string[];
  reuse: boolean;
  select: string[];
  replace: boolean;
};

export type Step = "create" | "same" | "conflict" | "incompatible";

export type Why =
  | "symlink_on_windows"
  | "link_leaves_folder"
  | "name_character"
  | "name_trailing"
  | "name_reserved"
  | "same_name_other_case"
  | "folder_in_the_way"
  | "link_in_the_way"
  | "not_a_link_there";

export type FileStep = {
  archive: string;
  dest: string;
  step: Step;
  why: Why | null;
  other: string | null;
};

export type LinkStep = { dir: string; account: string; step: Step };

export type Note =
  | { kind: "project_dir_renamed"; account: string; from: string; to: string }
  | { kind: "project_dir_unmapped"; account: string; name: string }
  | { kind: "claude_json_paths_mapped"; account: string; keys: number }
  | { kind: "needs_review"; path: string; field: string }
  | { kind: "link_unmapped"; dir: string }
  | { kind: "login_needed"; account: string }
  | { kind: "reinstall"; path: string };

export type Plan = {
  accounts: { from: string; to: string; new: boolean; whole: boolean }[];
  projects: { from: string; to: string }[];
  left_out: string[];
  counts: { create: number; same: number; conflict: number; incompatible: number };
  files: FileStep[];
  links: LinkStep[];
  default_account: LinkStep | null;
  notes: Note[];
  replace: boolean;
};

export type Applied = {
  written: number;
  replaced: number;
  skipped: number;
  switch_changes: number;
  originals: string | null;
};

export type BackupErrorCode =
  | "backup_io"
  | "backup_source_changed"
  | "backup_nothing_selected"
  | "backup_unknown_account"
  | "backup_wrong_passphrase"
  | "backup_corrupt"
  | "backup_unsupported_format"
  | "backup_unsafe_path"
  | "backup_checksum"
  | "backup_pending"
  | "backup_account_exists"
  | "backup_unmapped_project"
  | "backup_duplicate_target"
  | "backup_bad_map"
  | "backup_empty_passphrase"
  | "backup_passphrase_mismatch"
  | "backup_file_exists"
  | "backup_not_open"
  | "backup_failed";

/** How a backup command fails: a code, and the one name or path it is about. */
export type BackupFailure = { code: BackupErrorCode; subject: string | null };

export function isBackupFailure(e: unknown): e is BackupFailure {
  return (
    typeof e === "object" &&
    e != null &&
    typeof (e as { code?: unknown }).code === "string" &&
    (e as { code: string }).code.startsWith("backup_")
  );
}
