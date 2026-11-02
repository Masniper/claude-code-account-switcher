//! Moving a Claude Code environment to another machine: an encrypted backup
//! of accounts and of project support files, and a staged, journaled restore.
//!
//! # The archive
//!
//! A tar stream, encrypted as an [age](https://age-encryption.org) file with
//! a passphrase (scrypt). Nothing about the contents is readable without the
//! passphrase, the manifest included. The manifest is the tar's last entry
//! and lists every file with its size and SHA-256; a restore checks all of
//! them before it writes anything.
//!
//! ```text
//! accounts/<label>/config/<rel>      a config dir ("default" = ~/.claude)
//! accounts/default/home.claude.json  the standard account's ~/.claude.json
//! projects/<id>/<rel>                support files under a project root
//! switch/default.identity-lock.json  the standard account's pin
//! manifest.json
//! ```
//!
//! # What a config dir holds, and what travels
//!
//! Classified by top-level name, from two sets of names in Claude Code
//! 2.1.309: what it leaves out of a host config snapshot, and the
//! directories another of its walks skips (the set that starts with
//! `agent-memory`). Credentials never travel; runtime state, caches and
//! locks are left behind and reported; the rest, transcripts and memory
//! included, travels as it is. Transcripts are never rewritten.

use crate::config::AppConfig;
use crate::sessions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;

/// Written over a value that looked like a secret.
pub const REDACTED: &str = "<redacted by claude-acc backup>";

/// Credentials. Never put in an archive. From 2.1.309's state list, plus the
/// standard account's `.claude.json` fields handled in `redact_claude_json`.
const SECRET: &[&str] = &[
    ".credentials.json",
    ".session_ingress_token",
    "hfi-auth.json",
];

/// Machine-bound state, caches, locks and binaries: rebuilt on the target or
/// meaningless there. From the same 2.1.309 list, minus what is the user's
/// work (`projects`, `todos`, `file-history`, `history.jsonl`, `plans`).
const RUNTIME: &[&str] = &[
    ".claude.json.backup",
    "active-time.json",
    "antproto.json",
    "api-dumps",
    "backups",
    "bridge-spawn",
    "cache",
    "ccr",
    "chrome",
    "computer-use.lock",
    "daemon",
    "debug",
    "downloads",
    "dump-prompts",
    "feedback",
    "feedback-bundles",
    "file-transfers",
    "gh-pr-status-cache.json",
    "ide",
    "image-cache",
    "jobs",
    "local",
    "local-settings",
    "logs",
    "mcp-discovery-cache",
    "mcp-needs-auth-cache.json",
    "mcp-skill-archives",
    "paste-cache",
    "policy-limits.json",
    "project-settings",
    "remote",
    "remote-control",
    "remote-settings.json",
    "scratch",
    "seed-admin",
    "server-sessions.json",
    "server.lock",
    "session-env",
    "shares",
    "shell-snapshots",
    "startup-perf",
    "stats-cache.json",
    "statsig",
    "storage-v2",
    "systemd",
    "telemetry",
    "traces",
    "uploads",
    "usage-data",
    // Live session registries (`<pid>.json`), an MCP discovery cache, and
    // the daemon's status files (2.1.309).
    "sessions",
    "state",
    "daemon-auth-cooldown",
    "daemon-auth-status.json",
    "daemon.status.json",
    // This tool's own cache of the last identity read; derived from a token.
    ".account-info.json",
];

/// Installed plugins and their marketplaces are git clones and downloaded
/// code, tied to paths on this machine. Reinstalled on the target.
const REINSTALL: &[&str] = &["plugins", "cowork_plugins"];

/// Files Claude Code writes beside these, under the same name plus a suffix
/// (`.signature.json`, `.stamp.json`, `.synced-at`): server-issued state.
const RUNTIME_PREFIXES: &[&str] = &["policy-limits.json", "remote-settings.json"];

/// Under a project root, only these travel: the agent's support files, not
/// the source.
const PROJECT_FILES: &[&str] = &["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", ".mcp.json"];
const PROJECT_DIRS: &[&str] = &[".claude", ".agents"];
/// Inside those, directories that hold checkouts or dependencies, not state.
const PROJECT_SKIP_DIRS: &[&str] = &["worktrees", "node_modules", ".git"];
/// What a restore into a different project may take: reusable definitions,
/// never the source project's state.
const REUSABLE: &[&str] = &["skills", "agents", "rules", "commands"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub tool_version: String,
    pub created_at: u64,
    pub source_platform: String,
    pub source_home: String,
    pub accounts: Vec<AccountEntry>,
    pub switch: Option<SwitchState>,
    pub projects: Vec<ProjectEntry>,
    /// Which working directory each `projects/<name>` was named after, where
    /// it could be told: needed to rename it for the target's paths.
    pub project_dirs: Vec<ProjectDir>,
    pub files: Vec<FileEntry>,
    pub redactions: Vec<Redaction>,
    pub excluded: Vec<Excluded>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountEntry {
    pub label: String,
    pub source_dir: String,
    /// Only the `projects/` entries of the selected project roots, not the
    /// whole account.
    pub projects_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchState {
    pub default: Option<String>,
    pub links: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub source_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDir {
    pub account: String,
    pub name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// Set for a symlink: its target as written, never followed.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redaction {
    pub path: String,
    /// Where in the file, e.g. `mcpServers.github.env.GITHUB_TOKEN`. Never
    /// the value.
    pub field: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Credential,
    Runtime,
    Reinstall,
    /// A socket, FIFO or device: not a file.
    Special,
    /// A checkout or dependency tree inside project support files.
    Checkout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excluded {
    pub path: String,
    pub reason: Reason,
}

/// Why a backup or restore stopped. Carries paths, never file contents.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// A file changed while it was being read: the snapshot would be
    /// inconsistent.
    SourceChanged(PathBuf),
    NothingSelected,
    UnknownAccount(String),
    WrongPassphrase,
    /// Not an age file, truncated, or tampered with.
    Corrupt(String),
    UnsupportedFormat(u32),
    /// An archive entry would land outside the staging directory.
    UnsafePath(String),
    ChecksumMismatch(String),
    /// A restore left a journal: roll it back before restoring again.
    JournalPending(PathBuf),
    /// The destination account exists and was not made by this backup.
    AccountExists(String),
    UnmappedProject(String),
    /// Two source accounts were sent to one target account.
    DuplicateTarget(String),
    /// A `--map` that is not `FROM=TO`.
    BadMap(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// How a config dir entry is treated, by its first path component.
fn classify_config(rel: &Path) -> Option<Reason> {
    let first = rel.components().next()?.as_os_str().to_string_lossy();
    let name = first.as_ref();
    if SECRET.contains(&name) {
        Some(Reason::Credential)
    } else if RUNTIME.contains(&name) || RUNTIME_PREFIXES.iter().any(|p| name.starts_with(p)) {
        Some(Reason::Runtime)
    } else if REINSTALL.contains(&name) {
        Some(Reason::Reinstall)
    } else if is_lock_like(rel) {
        Some(Reason::Runtime)
    } else {
        None
    }
}

/// Locks, PIDs and sockets by name, wherever they sit.
fn is_lock_like(rel: &Path) -> bool {
    rel.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|n| n.ends_with(".lock") || n.ends_with(".pid") || n.ends_with(".sock"))
}

// ---------------------------------------------------------------------------
// Path mapping

/// `FROM=TO` prefix rewrites for paths recorded on the source machine. A
/// prefix matches whole components only, and the rest of the path is joined
/// with the target's separator, so `/Users/a/work=F:\work` turns
/// `/Users/a/work/x/y` into `F:\work\x\y`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMap(Vec<(String, String)>);

impl PathMap {
    pub fn parse(pairs: &[String]) -> std::result::Result<Self, String> {
        let mut out = Vec::new();
        for pair in pairs {
            let (from, to) = pair.split_once('=').ok_or_else(|| pair.clone())?;
            if from.is_empty() || to.is_empty() {
                return Err(pair.clone());
            }
            out.push((
                trim_separators(from).to_string(),
                trim_separators(to).to_string(),
            ));
        }
        // Longest prefix first, so a nested mapping wins.
        out.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        Ok(Self(out))
    }

    /// The target path for `path`, or `None` if no prefix covers it.
    pub fn map(&self, path: &str) -> Option<String> {
        let normalized = path.replace('\\', "/");
        self.0.iter().find_map(|(from, to)| {
            let from_n = from.replace('\\', "/");
            let rest = normalized.strip_prefix(&from_n)?;
            if !(rest.is_empty() || rest.starts_with('/')) {
                return None;
            }
            let sep = if is_windows_path(to) { "\\" } else { "/" };
            let tail: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
            Some(if tail.is_empty() {
                to.clone()
            } else {
                format!("{to}{sep}{}", tail.join(sep))
            })
        })
    }
}

fn trim_separators(s: &str) -> &str {
    let trimmed = s.trim_end_matches(['/', '\\']);
    // `/` and `C:\` are roots: keep them whole.
    if trimmed.is_empty() || trimmed.ends_with(':') {
        s
    } else {
        trimmed
    }
}

fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    p.contains('\\') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

// ---------------------------------------------------------------------------
// Windows name rules

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Character,
    TrailingDotOrSpace,
    Reserved,
}

/// Why a relative path cannot be written on Windows, if it cannot: a
/// reserved device name, a character Windows forbids, or a component ending
/// in a dot or a space.
pub fn windows_name_problem(rel: &str) -> Option<NameProblem> {
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    for part in rel.split('/') {
        if part
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\') || c < ' ')
        {
            return Some(NameProblem::Character);
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Some(NameProblem::TrailingDotOrSpace);
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        if RESERVED.contains(&stem.as_str()) {
            return Some(NameProblem::Reserved);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Sources

/// One account's config dir, as found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub label: String,
    pub config_dir: PathBuf,
    /// The standard account keeps its `.claude.json` beside `~/.claude`, in
    /// the home directory. Managed accounts keep theirs inside the config dir.
    pub home_json: Option<PathBuf>,
}

/// Every account this tool knows: the standard one as `default` (when its
/// directory exists), then the managed ones.
pub fn discover(config: &AppConfig, standard_dir: &Path, standard_json: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    if standard_dir.is_dir() {
        out.push(Source {
            label: sessions::DEFAULT_LABEL.to_string(),
            config_dir: standard_dir.to_path_buf(),
            home_json: Some(standard_json.to_path_buf()),
        });
    }
    for name in config.list_accounts().unwrap_or_default() {
        out.push(Source {
            config_dir: config.account_path(&name),
            label: name,
            home_json: None,
        });
    }
    out
}

/// What to put in a backup. Accounts are copied whole; project roots get
/// their support files, and every listed `session_accounts` contributes the
/// `projects/` entries of those roots. The three modes are which of the two
/// lists is empty.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// The source's home folder, recorded so settings naming it can be
    /// found on the target.
    pub home: PathBuf,
    pub accounts: Vec<Source>,
    pub projects: Vec<PathBuf>,
    pub session_accounts: Vec<Source>,
    pub switch: Option<(SwitchState, Option<PathBuf>)>,
}

/// One thing to archive.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    archive: String,
    source: PathBuf,
    link: Option<String>,
    /// Size when surveyed: a file that differs by the time it is read has
    /// changed since, and fails the backup.
    size: u64,
    /// Which redaction applies, if any.
    redact: Redact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Redact {
    None,
    ClaudeJson,
    Settings,
    McpJson,
}

/// What a backup would contain, without reading any file's contents.
#[derive(Debug, Default)]
pub struct Survey {
    items: Vec<Item>,
    pub excluded: Vec<Excluded>,
    pub project_dirs: Vec<ProjectDir>,
    pub files: usize,
    pub bytes: u64,
    /// Project roots that do not exist or hold no support files.
    pub empty_projects: Vec<String>,
}

pub fn survey(scope: &Scope) -> Result<Survey> {
    let mut s = Survey::default();
    for acc in &scope.accounts {
        walk_config(&mut s, acc, &acc.config_dir, None)?;
        if let Some(json) = &acc.home_json
            && json.is_file()
        {
            push(
                &mut s,
                format!("accounts/{}/home.claude.json", acc.label),
                json,
                Redact::ClaudeJson,
            )?;
        }
    }
    let wanted: BTreeSet<String> = scope
        .projects
        .iter()
        .map(|p| sessions::project_dir_name(&p.to_string_lossy()))
        .collect();
    for acc in &scope.session_accounts {
        // Already copied whole.
        if scope.accounts.iter().any(|a| a.label == acc.label) {
            continue;
        }
        let projects = sessions::projects_dir(&acc.config_dir);
        for name in &wanted {
            let dir = projects.join(name);
            if dir.is_dir() {
                walk_config(&mut s, acc, &acc.config_dir, Some(&dir))?;
            }
        }
    }
    for (i, root) in scope.projects.iter().enumerate() {
        let before = s.items.len();
        walk_project(&mut s, &format!("p{i}"), root)?;
        if s.items.len() == before {
            s.empty_projects.push(root.to_string_lossy().into_owned());
        }
    }
    if let Some((_, Some(lock))) = &scope.switch
        && lock.is_file()
    {
        push(
            &mut s,
            "switch/default.identity-lock.json".into(),
            lock,
            Redact::None,
        )?;
    }
    s.project_dirs = name_project_dirs(scope);
    Ok(s)
}

fn push(s: &mut Survey, archive: String, source: &Path, redact: Redact) -> Result<()> {
    let meta = fs::symlink_metadata(source)?;
    let link = if meta.file_type().is_symlink() {
        Some(fs::read_link(source)?.to_string_lossy().into_owned())
    } else {
        s.bytes += meta.len();
        None
    };
    s.files += 1;
    s.items.push(Item {
        archive,
        size: meta.len(),
        source: source.to_path_buf(),
        redact: if link.is_some() { Redact::None } else { redact },
        link,
    });
    Ok(())
}

/// Walk a config dir, or only `only` inside it, classifying as it goes.
fn walk_config(s: &mut Survey, acc: &Source, config_dir: &Path, only: Option<&Path>) -> Result<()> {
    let start = only.unwrap_or(config_dir);
    let mut stack = vec![start.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)?.collect::<io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(config_dir).expect("under the config dir");
            let rel_s = slash(rel);
            let shown = format!("accounts/{}/config/{rel_s}", acc.label);
            if let Some(reason) = classify_config(rel) {
                // One line for a whole excluded directory, not one per file.
                s.excluded.push(Excluded {
                    path: shown,
                    reason,
                });
                continue;
            }
            let ft = entry.file_type()?;
            if ft.is_dir() && entry.file_name() == "node_modules" {
                // Installed packages, often with native binaries: reinstalled.
                s.excluded.push(Excluded {
                    path: shown,
                    reason: Reason::Checkout,
                });
            } else if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() || ft.is_symlink() {
                let redact = match rel_s.as_str() {
                    ".claude.json" => Redact::ClaudeJson,
                    "settings.json" | "settings.local.json" => Redact::Settings,
                    _ => Redact::None,
                };
                push(s, shown, &path, redact)?;
            } else {
                s.excluded.push(Excluded {
                    path: shown,
                    reason: Reason::Special,
                });
            }
        }
    }
    Ok(())
}

/// The agent's support files under a project root: never its source.
fn walk_project(s: &mut Survey, id: &str, root: &Path) -> Result<()> {
    for name in PROJECT_FILES {
        let path = root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            let redact = if *name == ".mcp.json" {
                Redact::McpJson
            } else {
                Redact::None
            };
            push(s, format!("projects/{id}/{name}"), &path, redact)?;
        }
    }
    for name in PROJECT_DIRS {
        let top = root.join(name);
        if !top.is_dir() {
            continue;
        }
        let mut stack = vec![top];
        while let Some(dir) = stack.pop() {
            let mut entries: Vec<_> = fs::read_dir(&dir)?.collect::<io::Result<_>>()?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let path = entry.path();
                let rel = slash(path.strip_prefix(root).expect("under the root"));
                let shown = format!("projects/{id}/{rel}");
                let ft = entry.file_type()?;
                let fname = entry.file_name().to_string_lossy().into_owned();
                if ft.is_dir() && PROJECT_SKIP_DIRS.contains(&fname.as_str()) {
                    s.excluded.push(Excluded {
                        path: shown,
                        reason: Reason::Checkout,
                    });
                } else if ft.is_dir() {
                    stack.push(path);
                } else if is_lock_like(Path::new(&fname)) {
                    s.excluded.push(Excluded {
                        path: shown,
                        reason: Reason::Runtime,
                    });
                } else if ft.is_file() || ft.is_symlink() {
                    let redact = if fname == "settings.json" || fname == "settings.local.json" {
                        Redact::Settings
                    } else {
                        Redact::None
                    };
                    push(s, shown, &path, redact)?;
                } else {
                    s.excluded.push(Excluded {
                        path: shown,
                        reason: Reason::Special,
                    });
                }
            }
        }
    }
    Ok(())
}

fn slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Which working directory each `projects/<name>` came from, where that can
/// be told: from paths this machine already knows (project roots, bindings,
/// `.claude.json`'s `projects` keys), else from a transcript's own `cwd`
/// field. Read only; transcripts are never changed.
fn name_project_dirs(scope: &Scope) -> Vec<ProjectDir> {
    let mut known: BTreeSet<String> = scope
        .projects
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if let Some((switch, _)) = &scope.switch {
        known.extend(switch.links.iter().map(|(dir, _)| dir.clone()));
    }
    let all: Vec<&Source> = scope
        .accounts
        .iter()
        .chain(&scope.session_accounts)
        .collect();
    for acc in &all {
        let json = acc
            .home_json
            .clone()
            .unwrap_or_else(|| acc.config_dir.join(".claude.json"));
        if let Some(keys) = fs::read_to_string(json)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| v.get("projects").and_then(|p| p.as_object()).cloned())
        {
            known.extend(keys.keys().cloned());
        }
    }
    let by_name: HashMap<String, String> = known
        .into_iter()
        .map(|p| (sessions::project_dir_name(&p), p))
        .collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for acc in all {
        if !seen.insert(acc.label.clone()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(sessions::projects_dir(&acc.config_dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let cwd = by_name
                .get(&name)
                .cloned()
                .or_else(|| cwd_from_transcripts(&entry.path(), &name));
            if let Some(cwd) = cwd {
                out.push(ProjectDir {
                    account: acc.label.clone(),
                    name,
                    cwd,
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.account, &a.name).cmp(&(&b.account, &b.name)));
    out
}

/// The `cwd` field of the first transcript line that has one, if it names
/// this directory. Only the first lines of each transcript are read.
fn cwd_from_transcripts(dir: &Path, name: &str) -> Option<String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .find_map(|p| {
            let reader = BufReader::new(fs::File::open(p).ok()?);
            reader
                .lines()
                .take(50)
                .map_while(|l| l.ok())
                .find_map(|line| {
                    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
                    let cwd = v.get("cwd")?.as_str()?.to_string();
                    (sessions::project_dir_name(&cwd) == name).then_some(cwd)
                })
        })
}

// ---------------------------------------------------------------------------
// Secrets inside JSON files

/// An environment variable name that usually holds a secret.
fn secret_like(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "AUTH",
        "CREDENTIAL",
        "COOKIE",
    ]
    .iter()
    .any(|w| k.contains(w))
}

/// Remove credentials from a parsed JSON file, recording each field it
/// touched, never its value. Values are replaced, not deleted, where the
/// structure has to survive (an `env` map), and deleted where the field is
/// itself the credential.
fn redact(kind: Redact, v: &mut serde_json::Value, path: &str, out: &mut Vec<Redaction>) {
    let mut note = |field: String| {
        out.push(Redaction {
            path: path.to_string(),
            field,
        })
    };
    match kind {
        Redact::None => {}
        Redact::ClaudeJson => {
            if let Some(obj) = v.as_object_mut() {
                // The API key itself, and the signed-in identity, which a
                // login on the target writes afresh.
                for field in ["primaryApiKey", "oauthAccount"] {
                    if obj.remove(field).is_some() {
                        note(field.to_string());
                    }
                }
                redact_mcp(obj.get_mut("mcpServers"), "mcpServers", &mut note);
                if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
                    for (key, project) in projects.iter_mut() {
                        redact_mcp(
                            project.get_mut("mcpServers"),
                            &format!("projects[{key}].mcpServers"),
                            &mut note,
                        );
                    }
                }
            }
        }
        Redact::Settings => {
            if let Some(env) = v.get_mut("env").and_then(|e| e.as_object_mut()) {
                for (key, value) in env.iter_mut() {
                    if secret_like(key) && value.is_string() {
                        *value = REDACTED.into();
                        note(format!("env.{key}"));
                    }
                }
            }
        }
        Redact::McpJson => redact_mcp(v.get_mut("mcpServers"), "mcpServers", &mut note),
    }
}

/// Every server's `headers` (they carry auth) and its secret-looking `env`.
fn redact_mcp(servers: Option<&mut serde_json::Value>, at: &str, note: &mut impl FnMut(String)) {
    let Some(servers) = servers.and_then(|s| s.as_object_mut()) else {
        return;
    };
    for (name, server) in servers.iter_mut() {
        if let Some(headers) = server.get_mut("headers").and_then(|h| h.as_object_mut()) {
            for (key, value) in headers.iter_mut() {
                *value = REDACTED.into();
                note(format!("{at}.{name}.headers.{key}"));
            }
        }
        if let Some(env) = server.get_mut("env").and_then(|e| e.as_object_mut()) {
            for (key, value) in env.iter_mut() {
                if secret_like(key) {
                    *value = REDACTED.into();
                    note(format!("{at}.{name}.env.{key}"));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Writing the archive

/// What a file looked like before it was read, to tell if it changed while
/// it was.
fn stamp(path: &Path) -> io::Result<(u64, Option<std::time::SystemTime>)> {
    let m = fs::symlink_metadata(path)?;
    Ok((m.len(), m.modified().ok()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Hashing<R> {
    inner: R,
    hash: Sha256,
    read: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        self.read += n as u64;
        Ok(n)
    }
}

/// Encrypt `survey`'s files into `out`, which must not exist yet. Written to
/// a `.part` file first and renamed once complete, so a failed backup never
/// leaves something that looks like a finished one. `work_factor` overrides
/// age's scrypt cost, which otherwise targets about a second; tests lower it.
pub fn create(
    scope: &Scope,
    survey: Survey,
    passphrase: age::secrecy::SecretString,
    out: &Path,
    work_factor: Option<u8>,
) -> Result<Manifest> {
    let part = out.with_extension("part");
    let result = write_archive(scope, survey, passphrase, &part, work_factor);
    match result {
        Ok(manifest) => {
            if out.exists() {
                let _ = fs::remove_file(&part);
                return Err(Error::Io(io::Error::from(io::ErrorKind::AlreadyExists)));
            }
            fs::rename(&part, out)?;
            Ok(manifest)
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

fn write_archive(
    scope: &Scope,
    survey: Survey,
    passphrase: age::secrecy::SecretString,
    part: &Path,
    work_factor: Option<u8>,
) -> Result<Manifest> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(part)?;
    let mut recipient = age::scrypt::Recipient::new(passphrase);
    if let Some(log_n) = work_factor {
        recipient.set_work_factor(log_n);
    }
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|e| Error::Io(io::Error::other(e.to_string())))?;
    let writer = encryptor.wrap_output(io::BufWriter::new(file))?;
    let mut tar = tar::Builder::new(writer);
    tar.mode(tar::HeaderMode::Deterministic);

    let mut files = Vec::new();
    let mut redactions = Vec::new();
    for item in &survey.items {
        if let Some(target) = &item.link {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            h.set_mode(0o777);
            tar.append_link(&mut h, &item.archive, target)?;
            files.push(FileEntry {
                path: item.archive.clone(),
                size: 0,
                sha256: sha256_hex(target.as_bytes()),
                link: Some(target.clone()),
            });
            continue;
        }
        let before = stamp(&item.source)?;
        if before.0 != item.size {
            return Err(Error::SourceChanged(item.source.clone()));
        }
        let (bytes_written, sha) = if item.redact == Redact::None {
            let mut reader = Hashing {
                inner: fs::File::open(&item.source)?.take(before.0),
                hash: Sha256::new(),
                read: 0,
            };
            let mut h = file_header(before.0);
            tar.append_data(&mut h, &item.archive, &mut reader)?;
            if reader.read != before.0 {
                return Err(Error::SourceChanged(item.source.clone()));
            }
            (reader.read, hex(&reader.hash.finalize()))
        } else {
            let raw = fs::read(&item.source)?;
            let data = match serde_json::from_slice::<serde_json::Value>(&raw) {
                Ok(mut v) => {
                    redact(item.redact, &mut v, &item.archive, &mut redactions);
                    serde_json::to_vec_pretty(&v).map_err(|e| Error::Io(io::Error::other(e)))?
                }
                // Not JSON: Claude Code would not read it either. Carried as
                // it is, and said so.
                Err(_) => {
                    redactions.push(Redaction {
                        path: item.archive.clone(),
                        field: "(not JSON; not checked for secrets)".into(),
                    });
                    raw
                }
            };
            let mut h = file_header(data.len() as u64);
            tar.append_data(&mut h, &item.archive, data.as_slice())?;
            (data.len() as u64, sha256_hex(&data))
        };
        if stamp(&item.source)? != before {
            return Err(Error::SourceChanged(item.source.clone()));
        }
        files.push(FileEntry {
            path: item.archive.clone(),
            size: bytes_written,
            sha256: sha,
            link: None,
        });
    }

    let manifest = Manifest {
        format: FORMAT_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: now(),
        source_platform: std::env::consts::OS.to_string(),
        source_home: scope.home.to_string_lossy().into_owned(),
        accounts: scope
            .accounts
            .iter()
            .map(|a| AccountEntry {
                label: a.label.clone(),
                source_dir: a.config_dir.to_string_lossy().into_owned(),
                projects_only: false,
            })
            .chain(
                scope
                    .session_accounts
                    .iter()
                    .filter(|a| !scope.accounts.iter().any(|f| f.label == a.label))
                    .map(|a| AccountEntry {
                        label: a.label.clone(),
                        source_dir: a.config_dir.to_string_lossy().into_owned(),
                        projects_only: true,
                    }),
            )
            .collect(),
        switch: scope.switch.as_ref().map(|(s, _)| s.clone()),
        projects: scope
            .projects
            .iter()
            .enumerate()
            .map(|(i, p)| ProjectEntry {
                id: format!("p{i}"),
                source_root: p.to_string_lossy().into_owned(),
            })
            .collect(),
        project_dirs: survey.project_dirs,
        files,
        redactions,
        excluded: survey.excluded,
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| Error::Io(io::Error::other(e)))?;
    let mut h = file_header(json.len() as u64);
    tar.append_data(&mut h, "manifest.json", json.as_slice())?;
    let writer = tar.into_inner()?;
    let mut inner = writer.finish()?;
    inner.flush()?;
    inner.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(manifest)
}

fn file_header(size: u64) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_size(size);
    h.set_mode(0o600);
    h
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Reading the archive

/// An archive path that is safe to join under a directory: relative, made of
/// plain components only, with `/` separators.
fn safe_rel(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return None;
    }
    let p = Path::new(path);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Decrypt `archive` into `staging` (which must be empty or absent) and check
/// every file against the manifest. Nothing outside `staging` is written.
/// Symlinks are not created there; their targets come from the manifest.
pub fn open(
    archive: &Path,
    passphrase: age::secrecy::SecretString,
    staging: &Path,
) -> Result<Manifest> {
    let file = fs::File::open(archive)?;
    let decryptor =
        age::Decryptor::new(BufReader::new(file)).map_err(|e| Error::Corrupt(e.to_string()))?;
    let identity = age::scrypt::Identity::new(passphrase);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| match e {
            age::DecryptError::DecryptionFailed
            | age::DecryptError::KeyDecryptionFailed
            | age::DecryptError::NoMatchingKeys => Error::WrongPassphrase,
            other => Error::Corrupt(other.to_string()),
        })?;
    fs::create_dir_all(staging)?;
    let mut tar = tar::Archive::new(reader);
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut links: BTreeMap<String, String> = BTreeMap::new();
    let mut manifest_raw: Option<Vec<u8>> = None;
    for entry in tar.entries().map_err(|e| Error::Corrupt(e.to_string()))? {
        let mut entry = entry.map_err(|e| Error::Corrupt(e.to_string()))?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let rel = safe_rel(&name).ok_or_else(|| Error::UnsafePath(name.clone()))?;
        match entry.header().entry_type() {
            tar::EntryType::Regular if name == "manifest.json" => {
                let mut raw = Vec::new();
                entry
                    .read_to_end(&mut raw)
                    .map_err(|e| Error::Corrupt(e.to_string()))?;
                manifest_raw = Some(raw);
            }
            tar::EntryType::Regular => {
                let dest = staging.join(&rel);
                fs::create_dir_all(dest.parent().expect("has a parent"))?;
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&dest)?;
                let mut reader = Hashing {
                    inner: &mut entry,
                    hash: Sha256::new(),
                    read: 0,
                };
                io::copy(&mut reader, &mut out).map_err(|e| Error::Corrupt(e.to_string()))?;
                hashes.insert(name, hex(&reader.hash.finalize()));
            }
            tar::EntryType::Symlink => {
                let target = entry
                    .link_name()
                    .map_err(|e| Error::Corrupt(e.to_string()))?
                    .ok_or_else(|| Error::Corrupt(name.clone()))?
                    .to_string_lossy()
                    .into_owned();
                hashes.insert(name.clone(), sha256_hex(target.as_bytes()));
                links.insert(name, target);
            }
            _ => return Err(Error::UnsafePath(name)),
        }
    }
    // tar stops at its end-of-archive blocks; read the rest so age checks
    // the final chunk's authentication too, and a cut-off file is caught.
    io::copy(&mut tar.into_inner(), &mut io::sink()).map_err(|e| Error::Corrupt(e.to_string()))?;
    let raw = manifest_raw.ok_or_else(|| Error::Corrupt("no manifest".into()))?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|e| Error::Corrupt(e.to_string()))?;
    if manifest.format != FORMAT_VERSION {
        return Err(Error::UnsupportedFormat(manifest.format));
    }
    for f in &manifest.files {
        safe_rel(&f.path).ok_or_else(|| Error::UnsafePath(f.path.clone()))?;
        if hashes.remove(&f.path).as_deref() != Some(f.sha256.as_str())
            || links.get(&f.path) != f.link.as_ref()
        {
            return Err(Error::ChecksumMismatch(f.path.clone()));
        }
    }
    // Anything left was in the archive but not in the manifest.
    if let Some((extra, _)) = hashes.into_iter().next() {
        return Err(Error::ChecksumMismatch(extra));
    }
    Ok(manifest)
}

/// A short, stable name for a backup, used to recognise what an earlier
/// restore of the same backup created.
pub fn backup_id(manifest: &Manifest) -> String {
    let json = serde_json::to_vec(manifest).unwrap_or_default();
    sha256_hex(&json)[..16].to_string()
}

// ---------------------------------------------------------------------------
// Planning a restore

/// Where a restore writes: discovered on the target machine, never taken
/// from the archive.
pub struct Target {
    pub switch: AppConfig,
    pub standard_dir: PathBuf,
    pub standard_json: PathBuf,
}

impl Target {
    fn config_dir(&self, label: &str) -> PathBuf {
        if label == sessions::DEFAULT_LABEL {
            self.standard_dir.clone()
        } else {
            self.switch.account_path(label)
        }
    }

    /// Where an account's `.claude.json` lives on the target.
    fn claude_json(&self, label: &str) -> PathBuf {
        if label == sessions::DEFAULT_LABEL {
            self.standard_json.clone()
        } else {
            self.switch.account_path(label).join(".claude.json")
        }
    }

    pub fn journal(&self) -> PathBuf {
        self.switch.base_dir.join("restore-journal.json")
    }
}

#[derive(Debug, Clone, Default)]
pub struct RestoreRequest {
    pub map: PathMap,
    /// Source account label to target label. Only these accounts are read.
    pub accounts: Vec<(String, String)>,
    /// Source project root (as recorded) to target root.
    pub projects: Vec<(String, PathBuf)>,
    /// Restore each account whole. Off, only the restored projects'
    /// sessions and memory are taken from it, into the target account of
    /// that name, which may already exist.
    pub whole_accounts: bool,
    /// Reuse definitions in a different project: only skills, agents, rules
    /// and commands, never the source project's state, memory or sessions.
    pub reuse: bool,
    /// With `reuse`, limit to these (`skills/review`, `agents/x.md`, ...).
    pub select: Vec<String>,
    /// Replace conflicting files, keeping each original in a backup folder.
    pub replace: bool,
    /// Check names against Windows' rules (true when restoring on Windows).
    pub windows_target: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Create,
    /// Already there with the same contents: nothing to do.
    Same,
    /// Something else is there. Skipped unless `replace`.
    Conflict,
    /// Cannot be written as it is on the target.
    Incompatible(Why),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Creating one needs Developer Mode or admin rights.
    SymlinkOnWindows,
    /// A link that points outside the folder it was backed up from.
    LinkLeavesFolder,
    Name(NameProblem),
    /// Another file in this restore differs from it only by case.
    SameNameOtherCase(String),
    FolderInTheWay,
    LinkInTheWay,
    /// A link is to go where a file or another link already is.
    NotALinkThere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub archive: String,
    pub dest: PathBuf,
    pub action: Action,
    /// Staged file to copy from: the extracted file, or a rewritten copy.
    source: PathBuf,
    link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// `projects/<from>` restored as `projects/<to>`, for the target's path.
    ProjectDirRenamed {
        account: String,
        from: String,
        to: String,
    },
    /// No mapping covered this project directory's path: kept its name, so
    /// Claude Code on the target will not find it for any directory.
    ProjectDirUnmapped {
        account: String,
        name: String,
    },
    /// `.claude.json`'s `projects` keys rewritten for the target's paths.
    ClaudeJsonPathsMapped {
        account: String,
        keys: usize,
    },
    /// A setting holds a command or path written for the source machine.
    NeedsReview {
        path: String,
        field: String,
    },
    /// A directory binding whose path no mapping covers.
    LinkUnmapped {
        dir: String,
    },
    LoginNeeded {
        account: String,
    },
    Reinstall {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOp {
    pub dir: String,
    pub account: String,
    pub action: Action,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub links: Vec<LinkOp>,
    pub default: Option<(String, Action)>,
    pub notes: Vec<Note>,
    /// Managed accounts this restore creates.
    pub new_accounts: Vec<String>,
}

impl Plan {
    pub fn count(&self, action: &Action) -> usize {
        self.ops.iter().filter(|o| &o.action == action).count()
    }
    pub fn incompatible(&self) -> impl Iterator<Item = &Op> {
        self.ops
            .iter()
            .filter(|o| matches!(o.action, Action::Incompatible(_)))
    }
}

const MARKER: &str = ".claude-acc-restored-from";

pub fn plan(
    manifest: &Manifest,
    staging: &Path,
    req: &RestoreRequest,
    target: &Target,
) -> Result<Plan> {
    let id = backup_id(manifest);
    let mut plan = Plan::default();
    let account_to: HashMap<&str, &str> = req
        .accounts
        .iter()
        .map(|(s, d)| (s.as_str(), d.as_str()))
        .collect();
    for (src, _) in &req.accounts {
        if !manifest.accounts.iter().any(|a| &a.label == src) {
            return Err(Error::UnknownAccount(src.clone()));
        }
    }
    let mut seen_dst = BTreeSet::new();
    for (_, dst) in &req.accounts {
        if !seen_dst.insert(dst.as_str()) {
            return Err(Error::DuplicateTarget(dst.clone()));
        }
    }
    let project_to: HashMap<&str, &Path> = req
        .projects
        .iter()
        .map(|(s, d)| (s.as_str(), d.as_path()))
        .collect();
    for (src, _) in &req.projects {
        if !manifest.projects.iter().any(|p| &p.source_root == src) {
            return Err(Error::UnmappedProject(src.clone()));
        }
    }

    // A whole account restores only into an account that is not there yet,
    // or one an earlier restore of this same backup made. Accounts are never
    // merged.
    for entry in &manifest.accounts {
        let Some(dst) = account_to.get(entry.label.as_str()) else {
            continue;
        };
        if *dst == sessions::DEFAULT_LABEL {
            continue;
        }
        let whole = req.whole_accounts && !entry.projects_only;
        let dir = target.switch.account_path(dst);
        if dir.exists() {
            let ours = fs::read_to_string(dir.join(MARKER)).is_ok_and(|m| m.trim() == id);
            if whole && !ours {
                return Err(Error::AccountExists((*dst).to_string()));
            }
        } else {
            plan.new_accounts.push((*dst).to_string());
            plan.notes.push(Note::LoginNeeded {
                account: (*dst).to_string(),
            });
        }
    }
    if req.whole_accounts && account_to.values().any(|d| *d == sessions::DEFAULT_LABEL) {
        plan.notes.push(Note::LoginNeeded {
            account: sessions::DEFAULT_LABEL.into(),
        });
    }

    // Each project directory's new name, for the target's path.
    let mut renames: HashMap<(String, String), String> = HashMap::new();
    for pd in &manifest.project_dirs {
        if !account_to.contains_key(pd.account.as_str()) {
            continue;
        }
        let new_cwd =
            project_target(&pd.cwd, manifest, &project_to).or_else(|| req.map.map(&pd.cwd));
        match new_cwd {
            Some(cwd) => {
                let to = sessions::project_dir_name(&cwd);
                if to != pd.name {
                    plan.notes.push(Note::ProjectDirRenamed {
                        account: pd.account.clone(),
                        from: pd.name.clone(),
                        to: to.clone(),
                    });
                }
                renames.insert((pd.account.clone(), pd.name.clone()), to);
            }
            None => plan.notes.push(Note::ProjectDirUnmapped {
                account: pd.account.clone(),
                name: pd.name.clone(),
            }),
        }
    }
    // A project dir whose path was never learned is unmapped too.
    for f in &manifest.files {
        if let Some((acc, name)) = project_dir_of(&f.path)
            && account_to.contains_key(acc)
            && !manifest
                .project_dirs
                .iter()
                .any(|p| p.account == acc && p.name == name)
        {
            let note = Note::ProjectDirUnmapped {
                account: acc.to_string(),
                name: name.to_string(),
            };
            if !plan.notes.contains(&note) {
                plan.notes.push(note);
            }
        }
    }
    // Sessions and memory of the selected projects, when only projects were
    // backed up: their directories, by name.
    let selected_project_dirs: BTreeSet<String> = manifest
        .project_dirs
        .iter()
        .filter(|pd| project_target(&pd.cwd, manifest, &project_to).is_some())
        .map(|pd| pd.name.clone())
        .collect();

    let mut taken: HashMap<String, String> = HashMap::new();
    for f in &manifest.files {
        let staged = staging.join(safe_rel(&f.path).expect("checked by open"));
        let Some((dest, mut source)) = placement(
            f,
            manifest,
            req,
            target,
            &account_to,
            &project_to,
            &renames,
            &selected_project_dirs,
        ) else {
            continue;
        };
        if source.as_os_str().is_empty() {
            source = staged.clone();
        }
        let mut action = action_for(&dest, &source, f.link.as_deref())?;
        // A link restores only if it stays inside what it came with.
        if let Some(target_s) = &f.link {
            if req.windows_target {
                action = Action::Incompatible(Why::SymlinkOnWindows);
            } else if !link_stays_inside(&f.path, target_s) {
                action = Action::Incompatible(Why::LinkLeavesFolder);
            }
        }
        if req.windows_target
            && let Some(problem) = windows_name_problem(&f.path)
        {
            action = Action::Incompatible(Why::Name(problem));
        }
        // Two paths that differ only by case land on one file on Windows and
        // on a default macOS volume.
        let key = dest.to_string_lossy().to_lowercase();
        if let Some(first) = taken.get(&key) {
            action = Action::Incompatible(Why::SameNameOtherCase(first.clone()));
        } else {
            taken.insert(key, f.path.clone());
        }
        // A rewrite of `.claude.json` is a staged copy, made here.
        plan.ops.push(Op {
            archive: f.path.clone(),
            dest,
            action,
            source,
            link: f.link.clone(),
        });
    }
    rewrite_claude_json(&mut plan, manifest, staging, req, &project_to)?;
    review_settings(&mut plan, manifest, req);
    for e in &manifest.excluded {
        if e.reason == Reason::Reinstall {
            plan.notes.push(Note::Reinstall {
                path: e.path.clone(),
            });
        }
    }
    if !req.reuse {
        plan_switch(&mut plan, manifest, req, target, &account_to, &project_to);
    }
    Ok(plan)
}

/// `accounts/<acc>/config/projects/<name>/...` → `(acc, name)`.
fn project_dir_of(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("accounts/")?;
    let (acc, rest) = rest.split_once('/')?;
    let rest = rest.strip_prefix("config/projects/")?;
    let name = rest.split('/').next()?;
    Some((acc, name))
}

/// The target path for a working directory that is, or is under, a project
/// root being restored to a chosen place.
fn project_target(
    cwd: &str,
    manifest: &Manifest,
    project_to: &HashMap<&str, &Path>,
) -> Option<String> {
    manifest.projects.iter().find_map(|p| {
        let dest = project_to.get(p.source_root.as_str())?;
        PathMap(vec![(
            p.source_root.clone(),
            dest.to_string_lossy().into_owned(),
        )])
        .map(cwd)
    })
}

/// Where an archived file goes, or `None` if this restore does not take it.
/// The second value is a replacement source, empty when the staged file is
/// used as it is.
#[allow(clippy::too_many_arguments)]
fn placement(
    f: &FileEntry,
    manifest: &Manifest,
    req: &RestoreRequest,
    target: &Target,
    account_to: &HashMap<&str, &str>,
    project_to: &HashMap<&str, &Path>,
    renames: &HashMap<(String, String), String>,
    selected_project_dirs: &BTreeSet<String>,
) -> Option<(PathBuf, PathBuf)> {
    if let Some(rest) = f.path.strip_prefix("accounts/") {
        if req.reuse {
            return None;
        }
        let (acc, rest) = rest.split_once('/')?;
        let dst = account_to.get(acc)?;
        let entry = manifest.accounts.iter().find(|a| a.label == acc)?;
        let whole = req.whole_accounts && !entry.projects_only;
        if !whole && !rest.starts_with("config/projects/") {
            return None;
        }
        if rest == "home.claude.json" {
            return Some((target.claude_json(dst), PathBuf::new()));
        }
        let rel = rest.strip_prefix("config/")?;
        // A managed account's `.claude.json` follows the target's layout.
        if rel == ".claude.json" && acc != sessions::DEFAULT_LABEL {
            return Some((target.claude_json(dst), PathBuf::new()));
        }
        let mut parts: Vec<String> = rel.split('/').map(String::from).collect();
        if parts.first().map(String::as_str) == Some("projects") && parts.len() > 1 {
            if !whole && !selected_project_dirs.contains(&parts[1]) {
                return None;
            }
            if let Some(new) = renames.get(&(acc.to_string(), parts[1].clone())) {
                parts[1] = new.clone();
            }
        }
        let mut dest = target.config_dir(dst);
        for p in &parts {
            dest.push(p);
        }
        return Some((dest, PathBuf::new()));
    }
    if let Some(rest) = f.path.strip_prefix("projects/") {
        let (id, rel) = rest.split_once('/')?;
        let project = manifest.projects.iter().find(|p| p.id == id)?;
        let root = project_to.get(project.source_root.as_str())?;
        if req.reuse && !reusable(rel, &req.select) {
            return None;
        }
        let mut dest = root.to_path_buf();
        for p in rel.split('/') {
            dest.push(p);
        }
        return Some((dest, PathBuf::new()));
    }
    if f.path == "switch/default.identity-lock.json" && !req.reuse && req.whole_accounts {
        let dst = account_to.get(sessions::DEFAULT_LABEL)?;
        if *dst == sessions::DEFAULT_LABEL {
            return Some((
                target.switch.base_dir.join("default.identity-lock.json"),
                PathBuf::new(),
            ));
        }
    }
    None
}

/// In reuse mode: definitions only, under `.claude/` or `.agents/`.
fn reusable(rel: &str, select: &[String]) -> bool {
    let Some(inner) = rel
        .strip_prefix(".claude/")
        .or_else(|| rel.strip_prefix(".agents/"))
    else {
        return false;
    };
    let kind = inner.split('/').next().unwrap_or("");
    if !REUSABLE.contains(&kind) {
        return false;
    }
    select.is_empty()
        || select.iter().any(|s| {
            let s = s.trim_end_matches('/');
            inner == s || inner.starts_with(&format!("{s}/"))
        })
}

/// Whether a relative link, resolved from where it sits, stays inside the
/// top folder it was archived under (a config dir or a project root).
fn link_stays_inside(archive_path: &str, target: &str) -> bool {
    if target.starts_with('/') || is_windows_path(target) {
        return false;
    }
    // Depth below the root: `accounts/<a>/config/...` and `projects/<id>/...`.
    let below_root: Vec<&str> = if let Some(r) = archive_path.strip_prefix("accounts/") {
        r.splitn(3, '/').nth(2).unwrap_or("").split('/').collect()
    } else if let Some(r) = archive_path.strip_prefix("projects/") {
        r.split_once('/')
            .map(|x| x.1)
            .unwrap_or("")
            .split('/')
            .collect()
    } else {
        return false;
    };
    let mut depth = below_root.len() as i64 - 1;
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => depth -= 1,
            _ => depth += 1,
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

fn action_for(dest: &Path, source: &Path, link: Option<&str>) -> Result<Action> {
    let Ok(meta) = fs::symlink_metadata(dest) else {
        return Ok(Action::Create);
    };
    if meta.is_dir() {
        return Ok(Action::Incompatible(Why::FolderInTheWay));
    }
    if let Some(link) = link {
        let same = meta.file_type().is_symlink()
            && fs::read_link(dest).is_ok_and(|t| t.to_string_lossy() == link);
        return Ok(if same {
            Action::Same
        } else {
            Action::Incompatible(Why::NotALinkThere)
        });
    }
    if meta.file_type().is_symlink() {
        return Ok(Action::Incompatible(Why::LinkInTheWay));
    }
    if source.is_file() && fs::read(dest)? == fs::read(source)? {
        Ok(Action::Same)
    } else {
        Ok(Action::Conflict)
    }
}

/// `.claude.json` keys its per-project state by absolute path. That key is
/// structure, not history, so it is rewritten for the target's paths; the
/// rewritten copy goes beside the staged one and the archive is untouched.
fn rewrite_claude_json(
    plan: &mut Plan,
    manifest: &Manifest,
    staging: &Path,
    req: &RestoreRequest,
    project_to: &HashMap<&str, &Path>,
) -> Result<()> {
    for op in &mut plan.ops {
        let is_json = op.archive.ends_with("/home.claude.json")
            || (op.archive.ends_with("/config/.claude.json")
                && !op
                    .archive
                    .starts_with(&format!("accounts/{}/", sessions::DEFAULT_LABEL)));
        if !is_json || matches!(op.action, Action::Incompatible(_)) {
            continue;
        }
        let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&fs::read(&op.source)?) else {
            continue;
        };
        let Some(projects) = v.get_mut("projects").and_then(|p| p.as_object_mut()) else {
            continue;
        };
        let mut mapped = 0;
        let old = std::mem::take(projects);
        for (key, value) in old {
            match project_target(&key, manifest, project_to).or_else(|| req.map.map(&key)) {
                Some(new) if new != key => {
                    mapped += 1;
                    projects.insert(new, value);
                }
                _ => {
                    projects.insert(key, value);
                }
            }
        }
        if mapped == 0 {
            continue;
        }
        let rewritten = staging
            .join(".rewritten")
            .join(safe_rel(&op.archive).expect("checked by open"));
        fs::create_dir_all(rewritten.parent().expect("has a parent"))?;
        let data = serde_json::to_vec_pretty(&v).map_err(|e| Error::Io(io::Error::other(e)))?;
        fs::write(&rewritten, data)?;
        op.source = rewritten;
        op.action = action_for(&op.dest, &op.source, None)?;
        let account = op.archive.split('/').nth(1).unwrap_or("").to_string();
        plan.notes.push(Note::ClaudeJsonPathsMapped {
            account,
            keys: mapped,
        });
    }
    Ok(())
}

/// Settings that run something — hooks, the status line, MCP servers, helper
/// scripts — name commands and paths of the source machine. They are not
/// rewritten (a command line is not structure); each one that names the
/// source's home, or any command at all when the platform changes, is
/// listed by field for the user to check.
fn review_settings(plan: &mut Plan, manifest: &Manifest, req: &RestoreRequest) {
    let cross = req.windows_target != (manifest.source_platform == "windows");
    for op in &plan.ops {
        let name = op.archive.rsplit('/').next().unwrap_or("");
        if !matches!(
            name,
            "settings.json"
                | "settings.local.json"
                | ".claude.json"
                | "home.claude.json"
                | ".mcp.json"
        ) || matches!(op.action, Action::Incompatible(_))
        {
            continue;
        }
        let Ok(v) = fs::read(&op.source)
            .map_err(|_| ())
            .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).map_err(|_| ()))
        else {
            continue;
        };
        let mut fields = Vec::new();
        machine_bound(&v, "", None, &manifest.source_home, cross, &mut fields);
        for field in fields {
            plan.notes.push(Note::NeedsReview {
                path: op.archive.clone(),
                field,
            });
        }
    }
}

fn machine_bound(
    v: &serde_json::Value,
    at: &str,
    key: Option<&str>,
    home: &str,
    cross: bool,
    out: &mut Vec<String>,
) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, child) in map {
                // `projects` keys are handled by the rewrite above.
                if at.is_empty() && k == "projects" {
                    continue;
                }
                let next = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                machine_bound(child, &next, Some(k), home, cross, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                machine_bound(child, &format!("{at}[{i}]"), key, home, cross, out);
            }
        }
        serde_json::Value::String(s) => {
            let names_home = !home.is_empty() && s.contains(home);
            let command =
                cross && matches!(key, Some("command" | "apiKeyHelper" | "awsAuthRefresh"));
            if names_home || command {
                out.push(at.to_string());
            }
        }
        _ => {}
    }
}

/// Directory bindings and the default account, mapped to the target's paths
/// and account names.
fn plan_switch(
    plan: &mut Plan,
    manifest: &Manifest,
    req: &RestoreRequest,
    target: &Target,
    account_to: &HashMap<&str, &str>,
    project_to: &HashMap<&str, &Path>,
) {
    let Some(sw) = &manifest.switch else { return };
    let existing = target.switch.all_links().unwrap_or_default();
    for (dir, acc) in &sw.links {
        let Some(dst) = account_to.get(acc.as_str()) else {
            continue;
        };
        let in_project = project_target(dir, manifest, project_to);
        if !req.whole_accounts && in_project.is_none() {
            continue;
        }
        let Some(new_dir) = in_project.or_else(|| req.map.map(dir)) else {
            plan.notes.push(Note::LinkUnmapped { dir: dir.clone() });
            continue;
        };
        let action = match existing.iter().find(|(d, _)| *d == new_dir) {
            None => Action::Create,
            Some((_, a)) if a == dst => Action::Same,
            Some(_) => Action::Conflict,
        };
        plan.links.push(LinkOp {
            dir: new_dir,
            account: (*dst).to_string(),
            action,
        });
    }
    if let Some(def) = &sw.default
        && req.whole_accounts
        && let Some(dst) = account_to.get(def.as_str())
    {
        let action = match target.switch.get_default().ok().flatten() {
            None => Action::Create,
            Some(cur) if cur == *dst => Action::Same,
            Some(_) => Action::Conflict,
        };
        plan.default = Some(((*dst).to_string(), action));
    }
}

// ---------------------------------------------------------------------------
// Applying a plan

/// What `apply` did.
#[derive(Debug, Default)]
pub struct Applied {
    pub written: usize,
    pub replaced: usize,
    /// Conflicts left alone, and anything that cannot be written here.
    pub skipped: usize,
    pub switch_changes: usize,
    /// The run's folder: its journal and the originals of replaced files.
    pub run_dir: Option<PathBuf>,
}

/// One step of a restore, logged before it is taken, so an interrupted
/// restore can be undone from the log alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Step {
    Dir(PathBuf),
    Created(PathBuf),
    Replaced { dest: PathBuf, saved: PathBuf },
    Done,
}

const JOURNAL: &str = "journal.jsonl";

fn runs_dir(target: &Target) -> PathBuf {
    target.switch.base_dir.join("restore-backups")
}

fn read_journal(run: &Path) -> Vec<Step> {
    let Ok(raw) = fs::read_to_string(run.join(JOURNAL)) else {
        return Vec::new();
    };
    // A line cut short by a crash is the one step that never started.
    raw.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The newest restore that neither finished nor was rolled back.
pub fn pending(target: &Target) -> Option<PathBuf> {
    let mut runs: Vec<PathBuf> = fs::read_dir(runs_dir(target))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(JOURNAL).is_file())
        .collect();
    runs.sort();
    runs.into_iter()
        .rev()
        .find(|r| read_journal(r).last() != Some(&Step::Done))
}

struct Journal {
    file: fs::File,
}

impl Journal {
    // Flushed to the OS, not synced to disk: it guards against the process
    // dying mid-restore, the failure that happens; an fsync per file would
    // make a large restore take minutes.
    fn log(&mut self, step: &Step) -> io::Result<()> {
        let mut line = serde_json::to_vec(step).map_err(io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()
    }
}

/// Write `plan` to the target: only `Create`, plus `Conflict` when
/// `replace`, whose originals are kept in the run's folder first. Every step
/// is journaled; on any failure the steps taken so far are undone before
/// the error is returned. `fail_after` stops after that many steps, for
/// tests of exactly that.
pub fn apply(
    plan: &Plan,
    manifest: &Manifest,
    target: &Target,
    replace: bool,
    fail_after: Option<usize>,
) -> Result<Applied> {
    if let Some(run) = pending(target) {
        return Err(Error::JournalPending(run));
    }
    let wanted = |a: &Action| *a == Action::Create || (replace && *a == Action::Conflict);
    let mut applied = Applied {
        skipped: plan
            .ops
            .iter()
            .filter(|o| !wanted(&o.action) && o.action != Action::Same)
            .count(),
        ..Applied::default()
    };
    let work = plan.ops.iter().any(|o| wanted(&o.action))
        || plan.links.iter().any(|l| wanted(&l.action))
        || plan.default.as_ref().is_some_and(|(_, a)| wanted(a))
        || !plan.new_accounts.is_empty();
    if !work {
        return Ok(applied);
    }
    let runs = runs_dir(target);
    fs::create_dir_all(&runs)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let run = runs.join(format!("{stamp:020}-{}", backup_id(manifest)));
    fs::create_dir(&run)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(run.join(JOURNAL))?;
    let mut journal = Journal { file };
    applied.run_dir = Some(run.clone());

    let result = apply_steps(
        plan,
        manifest,
        target,
        replace,
        fail_after,
        &run,
        &mut journal,
        &mut applied,
    );
    match result {
        Ok(()) => {
            journal.log(&Step::Done)?;
            Ok(applied)
        }
        Err(e) => {
            // Undo what was done. If that fails too, the journal stays
            // pending and `rollback` can be run again.
            let _ = rollback_run(&run);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_steps(
    plan: &Plan,
    manifest: &Manifest,
    target: &Target,
    replace: bool,
    fail_after: Option<usize>,
    run: &Path,
    journal: &mut Journal,
    applied: &mut Applied,
) -> Result<()> {
    let mut steps = 0usize;
    let mut tick = || -> Result<()> {
        steps += 1;
        if fail_after.is_some_and(|n| steps > n) {
            return Err(Error::Io(io::Error::other("stopped by test")));
        }
        Ok(())
    };
    let id = backup_id(manifest);
    for name in &plan.new_accounts {
        tick()?;
        let marker = target.switch.account_path(name).join(MARKER);
        make_parents(&marker, journal)?;
        journal.log(&Step::Created(marker.clone()))?;
        write_new(&marker, id.as_bytes())?;
    }
    let mut saved = 0usize;
    for op in &plan.ops {
        let replacing = replace && op.action == Action::Conflict;
        if op.action != Action::Create && !replacing {
            continue;
        }
        tick()?;
        make_parents(&op.dest, journal)?;
        if replacing {
            saved += 1;
            save_original(&op.dest, run, saved, journal)?;
            write_over(&op.dest, &op.source)?;
            applied.replaced += 1;
            continue;
        }
        journal.log(&Step::Created(op.dest.clone()))?;
        match &op.link {
            Some(to) => make_link(to, &op.dest)?,
            None => write_new(&op.dest, &fs::read(&op.source)?)?,
        }
        applied.written += 1;
    }
    // Bindings and the default go through the switch's own files; each is
    // saved once before its first change.
    let mut saved_links = false;
    for link in &plan.links {
        if !(link.action == Action::Create || (replace && link.action == Action::Conflict)) {
            continue;
        }
        tick()?;
        if !saved_links {
            saved += 1;
            keep_switch_file(&target.switch.links_path(), run, saved, journal)?;
            saved_links = true;
        }
        target.switch.set_link(&link.dir, &link.account)?;
        applied.switch_changes += 1;
    }
    if let Some((name, action)) = &plan.default
        && (*action == Action::Create || (replace && *action == Action::Conflict))
    {
        tick()?;
        saved += 1;
        keep_switch_file(&target.switch.config_path(), run, saved, journal)?;
        target.switch.set_default(name)?;
        applied.switch_changes += 1;
    }
    Ok(())
}

/// Create the missing folders above `path`, top down, logging each.
fn make_parents(path: &Path, journal: &mut Journal) -> Result<()> {
    let mut missing = Vec::new();
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.as_os_str().is_empty() || fs::symlink_metadata(d).is_ok() {
            break;
        }
        missing.push(d.to_path_buf());
        dir = d.parent();
    }
    for d in missing.into_iter().rev() {
        journal.log(&Step::Dir(d.clone()))?;
        fs::create_dir(&d)?;
    }
    Ok(())
}

fn write_new(dest: &Path, data: &[u8]) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)?;
    f.write_all(data)
}

/// Replace `dest` with `source` in one rename, so it is never half written.
fn write_over(dest: &Path, source: &Path) -> io::Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".claude-acc-restore");
    let tmp = PathBuf::from(tmp);
    fs::copy(source, &tmp)?;
    fs::rename(&tmp, dest)
}

fn save_original(dest: &Path, run: &Path, n: usize, journal: &mut Journal) -> Result<PathBuf> {
    let originals = run.join("originals");
    fs::create_dir_all(&originals)?;
    let saved = originals.join(n.to_string());
    journal.log(&Step::Replaced {
        dest: dest.to_path_buf(),
        saved: saved.clone(),
    })?;
    // Renamed into place only when whole: rollback trusts any copy it finds.
    let partial = originals.join(format!("{n}.partial"));
    fs::copy(dest, &partial)?;
    fs::rename(&partial, &saved)?;
    Ok(saved)
}

/// Before the switch's `links` or `config` changes: save it, or note that it
/// is new.
fn keep_switch_file(path: &Path, run: &Path, n: usize, journal: &mut Journal) -> Result<()> {
    if path.is_file() {
        save_original(path, run, n, journal)?;
    } else {
        make_parents(path, journal)?;
        journal.log(&Step::Created(path.to_path_buf()))?;
        // `set_link` reads the file before writing it.
        write_new(path, b"")?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_link(to: &str, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(to, at)
}

// The plan marks every link incompatible on Windows, so this is never
// reached there; it is an error rather than a silent skip if it ever is.
#[cfg(not(unix))]
fn make_link(_to: &str, _at: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlinks are not restored on Windows",
    ))
}

/// Undo the newest unfinished restore, if there is one.
pub fn rollback(target: &Target) -> Result<Option<PathBuf>> {
    let Some(run) = pending(target) else {
        return Ok(None);
    };
    rollback_run(&run)?;
    Ok(Some(run))
}

fn rollback_run(run: &Path) -> Result<()> {
    for step in read_journal(run).into_iter().rev() {
        match step {
            Step::Created(p) => {
                if fs::symlink_metadata(&p).is_ok() {
                    fs::remove_file(&p)?;
                }
            }
            Step::Replaced { dest, saved } => {
                // Logged before the copy: a missing copy means the original
                // was never touched.
                if saved.is_file() {
                    write_over(&dest, &saved)?;
                }
            }
            // Only if empty: something else may have been put there since.
            Step::Dir(d) => {
                let _ = fs::remove_dir(&d);
            }
            Step::Done => {}
        }
    }
    // Close the run, so it no longer counts as pending.
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(run.join(JOURNAL))?;
    // On a line of its own, even after a line a crash cut short.
    f.write_all(b"\n\"done\"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use age::secrecy::SecretString;

    // Every value below is made up. Strings containing FAKESECRET stand in
    // for credentials, so a leak can be searched for.
    const LEAK: &str = "FAKESECRET";

    fn scratch(what: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-backup-{what}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn pass() -> SecretString {
        SecretString::from("correct horse".to_string())
    }

    fn put(path: &Path, data: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// A home directory: `.claude`, `.claude.json` and `.claude-switch`.
    struct Machine {
        home: PathBuf,
        switch: AppConfig,
    }

    impl Machine {
        fn new(home: PathBuf) -> Self {
            fs::create_dir_all(&home).unwrap();
            let switch = AppConfig {
                base_dir: home.join(".claude-switch"),
            };
            Self { home, switch }
        }
        fn std_dir(&self) -> PathBuf {
            self.home.join(".claude")
        }
        fn std_json(&self) -> PathBuf {
            self.home.join(".claude.json")
        }
        fn target(&self) -> Target {
            Target {
                switch: AppConfig {
                    base_dir: self.switch.base_dir.clone(),
                },
                standard_dir: self.std_dir(),
                standard_json: self.std_json(),
            }
        }
        fn sources(&self) -> Vec<Source> {
            discover(&self.switch, &self.std_dir(), &self.std_json())
        }
    }

    /// Two accounts and one project, as a source machine would have them.
    fn source_machine(root: &Path) -> (Machine, PathBuf) {
        let m = Machine::new(root.join("mac"));
        let work = root.join("mac-work");
        let app = work.join("app");
        let slug = sessions::project_dir_name(&s(&app));
        let d = m.std_dir();
        put(
            &d.join("settings.json"),
            // Built with `json!` so a Windows path's `\` is escaped.
            &serde_json::json!({
                "env": {"ANTHROPIC_API_KEY": format!("sk-{LEAK}"), "EDITOR": "vim"},
                "statusLine": {"type": "command", "command": format!("{}/bin/line", s(&m.home))},
            })
            .to_string(),
        );
        put(
            &d.join(".credentials.json"),
            &format!(r#"{{"claudeAiOauth":{{"accessToken":"{LEAK}-1"}}}}"#),
        );
        put(&d.join("CLAUDE.md"), "be brief\n");
        put(
            &d.join("skills/review/SKILL.md"),
            "---\nname: review\n---\n",
        );
        put(&d.join("agents/helper.md"), "agent\n");
        put(&d.join("shell-snapshots/snap.sh"), "runtime\n");
        put(&d.join("plugins/installed_plugins.json"), "{}");
        put(&d.join("sessions/4242.json"), "{}");
        put(&d.join("policy-limits.json.signature.json"), "{}");
        put(&d.join("mcp-servers/own/node_modules/dep/index.js"), "x");
        put(&d.join("mcp-servers/own/index.js"), "server");
        put(
            &d.join(format!("projects/{slug}/s1.jsonl")),
            &(serde_json::json!({"cwd": s(&app), "type": "user"}).to_string() + "\n"),
        );
        put(
            &d.join(format!("projects/{slug}/s1/subagents/agent-a.jsonl")),
            "{\"type\":\"assistant\"}\n",
        );
        put(
            &d.join(format!("projects/{slug}/memory/MEMORY.md")),
            "- remembered\n",
        );
        put(
            &m.std_json(),
            &serde_json::json!({
                "oauthAccount": {"emailAddress": "x@example.invalid"},
                "primaryApiKey": format!("{LEAK}-2"),
                "projects": {s(&app): {"allowedTools": []}},
                "mcpServers": {"gh": {
                    "command": "npx",
                    "env": {"GITHUB_TOKEN": format!("{LEAK}-3")},
                    "headers": {"Authorization": format!("Bearer {LEAK}-4")},
                }},
            })
            .to_string(),
        );
        let w = m.switch.account_path("work");
        put(&w.join("settings.json"), r#"{"model":"opus"}"#);
        put(
            &w.join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"w@example.invalid"}}"#,
        );
        put(&w.join("skills/x/SKILL.md"), "x\n");
        put(
            &w.join(format!("projects/{slug}/s2.jsonl")),
            "{\"type\":\"user\"}\n",
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink("x", w.join("skills/linked")).unwrap();
        m.switch.init().unwrap();
        m.switch.set_link(&s(&app), "work").unwrap();
        m.switch.set_default("work").unwrap();

        put(&app.join("CLAUDE.md"), "project rules\n");
        put(&app.join(".claude/skills/deploy/SKILL.md"), "deploy\n");
        put(
            &app.join(".claude/settings.json"),
            r#"{"permissions":{"allow":[]}}"#,
        );
        put(&app.join(".claude/worktrees/wt/file"), "checkout\n");
        put(&app.join("src/main.rs"), "fn main() {}\n");
        (m, app)
    }

    fn full_scope(m: &Machine, app: &Path) -> Scope {
        let links = m.switch.all_links().unwrap();
        Scope {
            home: m.home.clone(),
            accounts: m.sources(),
            projects: vec![app.to_path_buf()],
            session_accounts: m.sources(),
            switch: Some((
                SwitchState {
                    default: m.switch.get_default().unwrap(),
                    links,
                },
                None,
            )),
        }
    }

    fn backup(scope: &Scope, out: &Path) -> Manifest {
        let survey = survey(scope).unwrap();
        create(scope, survey, pass(), out, Some(4)).unwrap()
    }

    fn all_files(root: &Path) -> BTreeSet<PathBuf> {
        let mut out = BTreeSet::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if e.file_type().unwrap().is_dir() {
                    stack.push(p);
                } else {
                    out.insert(p);
                }
            }
        }
        out
    }

    fn same_names(m: &Manifest) -> Vec<(String, String)> {
        m.accounts
            .iter()
            .map(|a| (a.label.clone(), a.label.clone()))
            .collect()
    }

    #[test]
    fn round_trip_restores_accounts_sessions_memory_and_bindings_under_new_paths() {
        let root = scratch("roundtrip");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        let manifest = backup(&full_scope(&src, &app), &archive);

        let dst = Machine::new(root.join("win"));
        let new_app = root.join("win-work").join("app");
        let staging = root.join("staging");
        let opened = open(&archive, pass(), &staging).unwrap();
        assert_eq!(opened, manifest);
        let req = RestoreRequest {
            map: PathMap::parse(&[format!(
                "{}={}",
                s(&root.join("mac-work")),
                s(&root.join("win-work"))
            )])
            .unwrap(),
            accounts: same_names(&opened),
            projects: vec![(s(&app), new_app.clone())],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let target = dst.target();
        let plan = plan(&opened, &staging, &req, &target).unwrap();
        let done = apply(&plan, &opened, &target, false, None).unwrap();
        assert!(done.written > 0);
        assert_eq!(done.replaced, 0);

        let new_slug = sessions::project_dir_name(&s(&new_app));
        let d = dst.std_dir();
        assert!(d.join(format!("projects/{new_slug}/s1.jsonl")).is_file());
        assert!(
            d.join(format!("projects/{new_slug}/s1/subagents/agent-a.jsonl"))
                .is_file()
        );
        assert_eq!(
            fs::read_to_string(d.join(format!("projects/{new_slug}/memory/MEMORY.md"))).unwrap(),
            "- remembered\n"
        );
        // The transcript is carried byte for byte: its old `cwd` included.
        assert!(
            fs::read_to_string(d.join(format!("projects/{new_slug}/s1.jsonl")))
                .unwrap()
                .contains(&s(&app))
        );
        assert!(d.join("skills/review/SKILL.md").is_file());
        assert!(d.join("agents/helper.md").is_file());
        assert!(d.join("CLAUDE.md").is_file());
        assert!(d.join("mcp-servers/own/index.js").is_file());
        for gone in [
            ".credentials.json",
            "shell-snapshots",
            "plugins",
            "sessions",
            "policy-limits.json.signature.json",
            "mcp-servers/own/node_modules",
        ] {
            assert!(!d.join(gone).exists(), "{gone} should stay behind");
        }
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(d.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], REDACTED);
        assert_eq!(settings["env"]["EDITOR"], "vim");

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dst.std_json()).unwrap()).unwrap();
        assert!(json.get("oauthAccount").is_none() && json.get("primaryApiKey").is_none());
        assert!(
            json["projects"].get(s(&new_app)).is_some(),
            "project key follows the new path"
        );
        assert!(json["projects"].get(s(&app)).is_none());

        let w = dst.switch.account_path("work");
        assert!(w.join("skills/x/SKILL.md").is_file());
        assert!(w.join(format!("projects/{new_slug}/s2.jsonl")).is_file());
        assert!(!w.join(".credentials.json").exists());
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(w.join("skills/linked")).unwrap(),
            Path::new("x")
        );
        assert_eq!(
            dst.switch.get_link(&s(&new_app)).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(dst.switch.get_default().unwrap().as_deref(), Some("work"));

        assert!(new_app.join("CLAUDE.md").is_file());
        assert!(new_app.join(".claude/skills/deploy/SKILL.md").is_file());
        assert!(
            !new_app.join("src").exists(),
            "source code is not part of a backup"
        );
        assert!(!new_app.join(".claude/worktrees").exists());
        assert!(opened.excluded.iter().any(|e| e.reason == Reason::Checkout));
        assert!(plan.notes.iter().any(
            |n| matches!(n, Note::NeedsReview { field, .. } if field == "statusLine.command")
        ));
        assert!(plan.notes.contains(&Note::Reinstall {
            path: "accounts/default/config/plugins".into()
        }));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_credential_reaches_the_archive_or_the_manifest() {
        let root = scratch("leak");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let staging = root.join("staging");
        let manifest = open(&archive, pass(), &staging).unwrap();
        for f in all_files(&staging) {
            let raw = fs::read(&f).unwrap();
            assert!(
                !String::from_utf8_lossy(&raw).contains(LEAK),
                "{} leaks",
                f.display()
            );
        }
        assert!(!serde_json::to_string(&manifest).unwrap().contains(LEAK));
        // The fields are named, so the user knows what to fill in again.
        let fields: Vec<&str> = manifest
            .redactions
            .iter()
            .map(|r| r.field.as_str())
            .collect();
        for want in [
            "env.ANTHROPIC_API_KEY",
            "primaryApiKey",
            "oauthAccount",
            "mcpServers.gh.env.GITHUB_TOKEN",
            "mcpServers.gh.headers.Authorization",
        ] {
            assert!(fields.contains(&want), "{want} missing from {fields:?}");
        }
        assert!(
            manifest
                .excluded
                .iter()
                .any(|e| e.reason == Reason::Credential && e.path.ends_with(".credentials.json"))
        );
        // Encrypted: not even file names are readable without the passphrase.
        let raw = fs::read(&archive).unwrap();
        assert!(!raw.windows(8).any(|w| w == b"SKILL.md"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn restoring_again_changes_nothing() {
        let root = scratch("rerun");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        let target = dst.target();
        let req = |m: &Manifest| RestoreRequest {
            accounts: same_names(m),
            projects: vec![(s(&app), root.join("win-app"))],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let m = open(&archive, pass(), &root.join("st1")).unwrap();
        let p = plan(&m, &root.join("st1"), &req(&m), &target).unwrap();
        apply(&p, &m, &target, false, None).unwrap();
        let before = all_files(&dst.home);

        let m = open(&archive, pass(), &root.join("st2")).unwrap();
        let again = plan(&m, &root.join("st2"), &req(&m), &target).unwrap();
        assert_eq!(again.count(&Action::Create), 0);
        assert_eq!(again.count(&Action::Conflict), 0);
        assert!(again.new_accounts.is_empty());
        let done = apply(&again, &m, &target, false, None).unwrap();
        assert_eq!(
            (done.written, done.replaced, done.switch_changes),
            (0, 0, 0)
        );
        assert!(done.run_dir.is_none());
        assert_eq!(all_files(&dst.home), before);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn conflicts_are_skipped_unless_replace_and_the_original_is_kept() {
        let root = scratch("conflict");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        put(&dst.std_dir().join("CLAUDE.md"), "mine\n");
        let target = dst.target();
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let req = RestoreRequest {
            accounts: vec![("default".into(), "default".into())],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &target).unwrap();
        let op = p
            .ops
            .iter()
            .find(|o| o.archive == "accounts/default/config/CLAUDE.md")
            .unwrap();
        assert_eq!(op.action, Action::Conflict);

        let done = apply(&p, &m, &target, false, None).unwrap();
        assert!(done.skipped >= 1);
        assert_eq!(
            fs::read_to_string(dst.std_dir().join("CLAUDE.md")).unwrap(),
            "mine\n"
        );

        // Everything else is in place now; only the conflict is left to do.
        let p = plan(&m, &staging, &req, &target).unwrap();
        let done = apply(&p, &m, &target, true, None).unwrap();
        assert_eq!(done.replaced, 1);
        assert_eq!(
            fs::read_to_string(dst.std_dir().join("CLAUDE.md")).unwrap(),
            "be brief\n"
        );
        let originals = done.run_dir.unwrap().join("originals");
        let kept: Vec<String> = all_files(&originals)
            .iter()
            .map(|f| fs::read_to_string(f).unwrap())
            .collect();
        assert_eq!(kept, vec!["mine\n".to_string()]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_restore_that_fails_midway_is_undone() {
        let root = scratch("midway");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        put(&dst.std_dir().join("CLAUDE.md"), "mine\n");
        put(&dst.switch.links_path(), "/elsewhere=other\n");
        let target = dst.target();
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let req = RestoreRequest {
            accounts: same_names(&m),
            projects: vec![(s(&app), root.join("win-app"))],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &target).unwrap();
        let before: BTreeMap<PathBuf, Vec<u8>> = all_files(&dst.home)
            .into_iter()
            .map(|f| (f.clone(), fs::read(&f).unwrap()))
            .collect();
        let steps = p.ops.len() + p.links.len() + 2;
        for stop in [1, 5, steps - 1] {
            let err = apply(&p, &m, &target, true, Some(stop)).unwrap_err();
            assert!(matches!(err, Error::Io(_)));
            assert!(pending(&target).is_none(), "undone, so nothing is pending");
            // Only the run folders (journal and saved originals) remain.
            let after: BTreeMap<PathBuf, Vec<u8>> = all_files(&dst.home)
                .into_iter()
                .filter(|f| !f.starts_with(runs_dir(&target)) && !f.ends_with(".config.lock"))
                .map(|f| (f.clone(), fs::read(&f).unwrap()))
                .collect();
            assert_eq!(after, before, "stopped after {stop}");
            assert!(!root.join("win-app").exists());
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_interrupted_restore_blocks_the_next_one_until_rolled_back() {
        let root = scratch("interrupted");
        let dst = Machine::new(root.join("win"));
        let target = dst.target();
        // What a killed process leaves: steps logged, no `done`.
        let run = runs_dir(&target).join("00000000000000000001-abc");
        let made = dst.home.join("made/file");
        put(&made, "half\n");
        let kept = dst.home.join("kept");
        put(&kept, "new\n");
        put(&run.join("originals/1"), "old\n");
        let steps = [
            Step::Dir(dst.home.join("made")),
            Step::Created(made.clone()),
            Step::Replaced {
                dest: kept.clone(),
                saved: run.join("originals/1"),
            },
        ];
        let mut journal: String = steps
            .iter()
            .map(|s| serde_json::to_string(s).unwrap() + "\n")
            .collect();
        journal.push_str("{\"created\":\"/cut-off"); // torn last line
        put(&run.join(JOURNAL), &journal);

        let empty = Manifest {
            format: FORMAT_VERSION,
            tool_version: String::new(),
            created_at: 0,
            source_platform: "macos".into(),
            source_home: String::new(),
            accounts: vec![],
            switch: None,
            projects: vec![],
            project_dirs: vec![],
            files: vec![],
            redactions: vec![],
            excluded: vec![],
        };
        let p = Plan {
            new_accounts: vec!["n".into()],
            ..Plan::default()
        };
        assert!(
            matches!(apply(&p, &empty, &target, false, None), Err(Error::JournalPending(r)) if r == run)
        );
        assert_eq!(rollback(&target).unwrap(), Some(run));
        assert!(!dst.home.join("made").exists());
        assert_eq!(fs::read_to_string(&kept).unwrap(), "old\n");
        assert!(pending(&target).is_none());
        assert_eq!(rollback(&target).unwrap(), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn mapping_to_a_windows_drive_renames_project_dirs_and_their_sidecars() {
        let root = scratch("windows");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let req = RestoreRequest {
            map: PathMap::parse(&[format!("{}=F:\\work", s(&root.join("mac-work")))]).unwrap(),
            accounts: same_names(&m),
            windows_target: true,
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        // Planned only: the destinations are Windows paths.
        let p = plan(&m, &staging, &req, &dst.target()).unwrap();
        let dest_of = |archive_suffix: &str| {
            p.ops
                .iter()
                .find(|o| o.archive.ends_with(archive_suffix))
                .map(|o| s(&o.dest).replace('\\', "/"))
                .unwrap()
        };
        let sidecar = dest_of("s1/subagents/agent-a.jsonl");
        assert!(
            sidecar.contains("projects/F--work-app/s1/subagents/agent-a.jsonl"),
            "{sidecar}"
        );
        assert!(dest_of("memory/MEMORY.md").contains("projects/F--work-app/memory/"));
        let old = sessions::project_dir_name(&s(&app));
        assert!(p.notes.contains(&Note::ProjectDirRenamed {
            account: "work".into(),
            from: old.clone(),
            to: "F--work-app".into()
        }));
        assert!(
            p.links
                .iter()
                .any(|l| l.dir == "F:\\work\\app" && l.account == "work")
        );
        // On Windows the status line command is listed for review.
        assert!(p.notes.iter().any(
            |n| matches!(n, Note::NeedsReview { field, .. } if field == "statusLine.command")
        ));
        #[cfg(unix)]
        assert!(
            p.ops.iter().any(|o| o.archive.ends_with("skills/linked")
                && matches!(o.action, Action::Incompatible(_)))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    fn manifest_of(files: &[&str]) -> Manifest {
        Manifest {
            format: FORMAT_VERSION,
            tool_version: String::new(),
            created_at: 0,
            source_platform: "macos".into(),
            source_home: "/Users/m".into(),
            accounts: vec![AccountEntry {
                label: "a".into(),
                source_dir: "/Users/m/.claude-switch/accounts/a".into(),
                projects_only: false,
            }],
            switch: None,
            projects: vec![],
            project_dirs: vec![],
            files: files
                .iter()
                .map(|f| FileEntry {
                    path: format!("accounts/a/config/{f}"),
                    size: 0,
                    sha256: String::new(),
                    link: None,
                })
                .collect(),
            redactions: vec![],
            excluded: vec![],
        }
    }

    #[test]
    fn names_windows_cannot_hold_and_case_collisions_are_incompatible() {
        let root = scratch("names");
        let dst = Machine::new(root.join("win"));
        let m = manifest_of(&[
            "skills/a:b.md",
            "skills/aux.md",
            "skills/trail.",
            "skills/Readme.md",
            "skills/README.md",
            "skills/ok.md",
        ]);
        let req = RestoreRequest {
            accounts: vec![("a".into(), "a".into())],
            windows_target: true,
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &root.join("staging"), &req, &dst.target()).unwrap();
        let action = |name: &str| {
            p.ops
                .iter()
                .find(|o| o.archive.ends_with(name))
                .unwrap()
                .action
                .clone()
        };
        for bad in ["a:b.md", "aux.md", "trail.", "README.md"] {
            assert!(matches!(action(bad), Action::Incompatible(_)), "{bad}");
        }
        assert_eq!(action("Readme.md"), Action::Create);
        assert_eq!(action("ok.md"), Action::Create);
        // The same names are fine for a macOS or Linux target, case aside.
        let p = plan(
            &m,
            &root.join("staging"),
            &RestoreRequest {
                windows_target: false,
                ..req
            },
            &dst.target(),
        )
        .unwrap();
        assert_eq!(p.incompatible().count(), 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn links_restore_only_when_they_stay_inside_their_folder() {
        assert!(link_stays_inside("accounts/a/config/skills/linked", "x"));
        assert!(link_stays_inside(
            "accounts/a/config/skills/linked",
            "../agents/y"
        ));
        assert!(!link_stays_inside(
            "accounts/a/config/skills/linked",
            "../../outside"
        ));
        assert!(!link_stays_inside(
            "projects/p0/.claude/skills/s",
            "/Users/m/elsewhere"
        ));
        assert!(link_stays_inside(
            "projects/p0/.claude/skills/s",
            "../../CLAUDE.md"
        ));
        assert!(!link_stays_inside("projects/p0/CLAUDE.md", "../AGENTS.md"));
    }

    #[test]
    fn an_existing_account_is_never_merged_into() {
        let root = scratch("merge");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        put(&dst.switch.account_path("work").join("settings.json"), "{}");
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let into = |to: &str| RestoreRequest {
            accounts: vec![("work".into(), to.into())],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        assert!(
            matches!(plan(&m, &staging, &into("work"), &dst.target()), Err(Error::AccountExists(a)) if a == "work")
        );
        let p = plan(&m, &staging, &into("work-mac"), &dst.target()).unwrap();
        assert_eq!(p.new_accounts, vec!["work-mac".to_string()]);
        let two = RestoreRequest {
            accounts: vec![("work".into(), "x".into()), ("default".into(), "x".into())],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        assert!(matches!(
            plan(&m, &staging, &two, &dst.target()),
            Err(Error::DuplicateTarget(_))
        ));
        assert!(matches!(
            plan(
                &m,
                &staging,
                &into("work-mac").clone_with("nope"),
                &dst.target()
            ),
            Err(Error::UnknownAccount(_))
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    impl RestoreRequest {
        fn clone_with(&self, source: &str) -> Self {
            let mut r = self.clone();
            r.accounts[0].0 = source.into();
            r
        }
    }

    #[test]
    fn a_project_only_backup_carries_its_sessions_but_not_the_accounts() {
        let root = scratch("projectonly");
        let (src, app) = source_machine(&root);
        let scope = Scope {
            projects: vec![app.clone()],
            session_accounts: src.sources(),
            ..Scope::default()
        };
        let archive = root.join("p.ccbackup");
        let m = backup(&scope, &archive);
        assert!(m.accounts.iter().all(|a| a.projects_only));
        let slug = sessions::project_dir_name(&s(&app));
        for f in m.files.iter().filter(|f| f.path.starts_with("accounts/")) {
            assert!(
                f.path.contains(&format!("/config/projects/{slug}/")),
                "{}",
                f.path
            );
        }
        // Restored into a folder of the user's choosing: sessions follow it.
        let dst = Machine::new(root.join("win"));
        put(&dst.switch.account_path("work").join("settings.json"), "{}");
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let chosen = root.join("chosen");
        let req = RestoreRequest {
            accounts: same_names(&m),
            projects: vec![(s(&app), chosen.clone())],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &dst.target()).unwrap();
        apply(&p, &m, &dst.target(), false, None).unwrap();
        let new_slug = sessions::project_dir_name(&s(&chosen));
        assert!(
            dst.std_dir()
                .join(format!("projects/{new_slug}/s1/subagents/agent-a.jsonl"))
                .is_file()
        );
        assert!(
            dst.switch
                .account_path("work")
                .join(format!("projects/{new_slug}/s2.jsonl"))
                .is_file()
        );
        assert!(chosen.join(".claude/skills/deploy/SKILL.md").is_file());
        assert!(!dst.std_dir().join("skills").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn moving_one_project_out_of_a_full_backup_takes_only_its_sessions() {
        let root = scratch("oneproject");
        let (src, app) = source_machine(&root);
        let other = sessions::project_dir_name("/somewhere/else");
        put(
            &src.std_dir().join(format!("projects/{other}/o.jsonl")),
            "{}\n",
        );
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        // The target already has a `work` account: a project move adds to it.
        put(&dst.switch.account_path("work").join("settings.json"), "{}");
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let chosen = root.join("chosen");
        let req = RestoreRequest {
            accounts: same_names(&m),
            projects: vec![(s(&app), chosen.clone())],
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &dst.target()).unwrap();
        apply(&p, &m, &dst.target(), false, None).unwrap();
        let new_slug = sessions::project_dir_name(&s(&chosen));
        assert!(
            dst.std_dir()
                .join(format!("projects/{new_slug}/s1.jsonl"))
                .is_file()
        );
        assert!(
            dst.switch
                .account_path("work")
                .join(format!("projects/{new_slug}/s2.jsonl"))
                .is_file()
        );
        assert!(!dst.std_dir().join(format!("projects/{other}")).exists());
        assert!(!dst.std_dir().join("skills").exists() && !dst.std_json().exists());
        assert_eq!(
            fs::read_to_string(dst.switch.account_path("work").join("settings.json")).unwrap(),
            "{}"
        );
        // Its binding moves with it; the default account does not.
        assert_eq!(
            dst.switch.get_link(&s(&chosen)).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(dst.switch.get_default().unwrap(), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reuse_takes_only_the_selected_definitions() {
        let root = scratch("reuse");
        let (src, app) = source_machine(&root);
        put(&app.join(".claude/agents/rev.md"), "agent\n");
        put(&app.join(".claude/rules/style.md"), "rule\n");
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let other = root.join("other-project");
        let req = RestoreRequest {
            accounts: same_names(&m),
            projects: vec![(s(&app), other.clone())],
            reuse: true,
            select: vec!["skills/deploy".into(), "rules".into()],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &dst.target()).unwrap();
        apply(&p, &m, &dst.target(), false, None).unwrap();
        let got: BTreeSet<PathBuf> = all_files(&other)
            .into_iter()
            .map(|f| f.strip_prefix(&other).unwrap().to_path_buf())
            .collect();
        let want: BTreeSet<PathBuf> = [".claude/skills/deploy/SKILL.md", ".claude/rules/style.md"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(got, want);
        assert!(!dst.std_dir().exists(), "no account state in reuse mode");
        assert!(dst.switch.get_link(&s(&other)).unwrap_or(None).is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_dry_run_writes_nothing_to_the_target() {
        let root = scratch("dryrun");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let dst = Machine::new(root.join("win"));
        put(&dst.std_dir().join("CLAUDE.md"), "mine\n");
        let before = all_files(&root.join("win"));
        let staging = root.join("staging");
        let m = open(&archive, pass(), &staging).unwrap();
        let req = RestoreRequest {
            accounts: same_names(&m),
            projects: vec![(s(&app), root.join("dest"))],
            whole_accounts: true,
            ..RestoreRequest::default()
        };
        let p = plan(&m, &staging, &req, &dst.target()).unwrap();
        assert!(p.count(&Action::Create) > 0 && p.count(&Action::Conflict) == 1);
        assert_eq!(all_files(&root.join("win")), before);
        assert!(!root.join("dest").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_that_changes_during_backup_fails_it() {
        let root = scratch("changed");
        let (src, app) = source_machine(&root);
        let scope = full_scope(&src, &app);
        let survey = survey(&scope).unwrap();
        let file = src.std_dir().join("agents/helper.md");
        fs::write(&file, "agent, edited\n").unwrap();
        let out = root.join("env.ccbackup");
        let err = create(&scope, survey, pass(), &out, Some(4)).unwrap_err();
        assert!(matches!(err, Error::SourceChanged(p) if p == file));
        assert!(!out.exists() && !out.with_extension("part").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_backup_leaves_its_sources_untouched() {
        let root = scratch("readonly");
        let (src, app) = source_machine(&root);
        let snapshot = |r: &Path| -> BTreeMap<PathBuf, Vec<u8>> {
            all_files(r)
                .into_iter()
                .map(|f| match fs::read_link(&f) {
                    Ok(to) => (f, to.to_string_lossy().into_owned().into_bytes()),
                    Err(_) => (f.clone(), fs::read(&f).unwrap()),
                })
                .collect()
        };
        let before = snapshot(&root);
        backup(&full_scope(&src, &app), &root.join("env.ccbackup"));
        let mut after = snapshot(&root);
        after.remove(&root.join("env.ccbackup"));
        assert_eq!(after, before);
        fs::remove_dir_all(&root).unwrap();
    }

    // -- archives that must be refused --

    /// A tar made of raw entries, names written as given.
    fn tar_of(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut h = file_header(data.len() as u64);
            let gnu = h.as_gnu_mut().unwrap();
            gnu.name[..name.len()].copy_from_slice(name);
            h.set_cksum();
            tar.append(&h, *data).unwrap();
        }
        tar.into_inner().unwrap()
    }

    fn encrypt(out: &Path, plain: &[u8], passphrase: &str) {
        let mut recipient = age::scrypt::Recipient::new(SecretString::from(passphrase.to_string()));
        recipient.set_work_factor(4);
        let mut w =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .unwrap()
                .wrap_output(fs::File::create(out).unwrap())
                .unwrap();
        w.write_all(plain).unwrap();
        w.finish().unwrap();
    }

    fn seal(out: &Path, entries: &[(&[u8], &[u8])], passphrase: &str) {
        encrypt(out, &tar_of(entries), passphrase);
    }

    fn manifest_json(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut m = manifest_of(&[]);
        m.files = files
            .iter()
            .map(|(p, d)| FileEntry {
                path: p.to_string(),
                size: d.len() as u64,
                sha256: sha256_hex(d),
                link: None,
            })
            .collect();
        serde_json::to_vec(&m).unwrap()
    }

    #[test]
    fn a_wrong_passphrase_is_reported_as_one() {
        let root = scratch("wrongpass");
        let archive = root.join("a.ccbackup");
        seal(
            &archive,
            &[(b"manifest.json", &manifest_json(&[]))],
            "right",
        );
        let err = open(
            &archive,
            SecretString::from("wrong".to_string()),
            &root.join("st"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::WrongPassphrase), "{err:?}");
        assert!(
            open(
                &archive,
                SecretString::from("right".to_string()),
                &root.join("st2")
            )
            .is_ok()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_damaged_or_cut_off_archive_is_refused() {
        let root = scratch("damaged");
        let (src, app) = source_machine(&root);
        let archive = root.join("env.ccbackup");
        backup(&full_scope(&src, &app), &archive);
        let raw = fs::read(&archive).unwrap();
        let mut flipped = raw.clone();
        let mid = raw.len() / 2;
        flipped[mid] ^= 1;
        fs::write(root.join("flipped"), &flipped).unwrap();
        assert!(matches!(
            open(&root.join("flipped"), pass(), &root.join("s1")),
            Err(Error::Corrupt(_))
        ));
        fs::write(root.join("cut"), &raw[..raw.len() - 100]).unwrap();
        assert!(matches!(
            open(&root.join("cut"), pass(), &root.join("s2")),
            Err(Error::Corrupt(_))
        ));
        fs::write(root.join("plain"), b"not an archive").unwrap();
        assert!(matches!(
            open(&root.join("plain"), pass(), &root.join("s3")),
            Err(Error::Corrupt(_))
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_archive_missing_its_last_chunk_is_refused() {
        // age encrypts in 64 KiB chunks and marks the last one. Two full
        // chunks, the tar ending early in the first: dropping the second
        // leaves a file whose tar reads complete, so only reading the stream
        // to its end can tell it was cut.
        const CHUNK: usize = 64 * 1024;
        let root = scratch("lastchunk");
        let archive = root.join("a.ccbackup");
        let mut plain = tar_of(&[(b"manifest.json", &manifest_json(&[]))]);
        plain.resize(2 * CHUNK, 0);
        encrypt(&archive, &plain, "pw");
        assert!(
            open(
                &archive,
                SecretString::from("pw".to_string()),
                &root.join("whole")
            )
            .is_ok()
        );
        let raw = fs::read(&archive).unwrap();
        fs::write(&archive, &raw[..raw.len() - (CHUNK + 16)]).unwrap();
        let err = open(
            &archive,
            SecretString::from("pw".to_string()),
            &root.join("cut"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Corrupt(_)), "{err:?}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_that_does_not_match_its_checksum_is_refused() {
        let root = scratch("checksum");
        let archive = root.join("a.ccbackup");
        let manifest = manifest_json(&[("accounts/a/config/x", b"original")]);
        seal(
            &archive,
            &[
                (b"accounts/a/config/x", b"tampered"),
                (b"manifest.json", &manifest),
            ],
            "pw",
        );
        let err = open(
            &archive,
            SecretString::from("pw".to_string()),
            &root.join("st"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::ChecksumMismatch(p) if p == "accounts/a/config/x"));
        // An entry the manifest does not list is refused too.
        let manifest = manifest_json(&[]);
        seal(
            &archive,
            &[
                (b"accounts/a/config/extra", b"x"),
                (b"manifest.json", &manifest),
            ],
            "pw",
        );
        let err = open(
            &archive,
            SecretString::from("pw".to_string()),
            &root.join("st2"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::ChecksumMismatch(_)));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entries_that_escape_the_staging_folder_are_refused() {
        let root = scratch("traversal");
        let archive = root.join("a.ccbackup");
        let staging = root.join("deep/staging");
        for name in [
            &b"../../escaped"[..],
            b"/tmp/cc-backup-absolute",
            b"accounts/../../escaped",
            b"a\\..\\escaped",
        ] {
            seal(
                &archive,
                &[(name, b"x"), (b"manifest.json", &manifest_json(&[]))],
                "pw",
            );
            let err = open(&archive, SecretString::from("pw".to_string()), &staging).unwrap_err();
            assert!(
                matches!(err, Error::UnsafePath(_)),
                "{}: {err:?}",
                String::from_utf8_lossy(name)
            );
        }
        assert!(!root.join("escaped").exists() && !root.join("deep/escaped").exists());
        assert!(!Path::new("/tmp/cc-backup-absolute").exists());
        // And a manifest naming such a path is refused before any restore.
        assert!(
            safe_rel("../x").is_none() && safe_rel("/x").is_none() && safe_rel("a/./b").is_some()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn path_maps_match_whole_components_and_use_the_targets_separator() {
        let map = PathMap::parse(&[
            "/Users/m/work=F:\\work".into(),
            "/Users/m/work/special=G:\\s".into(),
        ])
        .unwrap();
        assert_eq!(
            map.map("/Users/m/work/app/sub").as_deref(),
            Some("F:\\work\\app\\sub")
        );
        assert_eq!(map.map("/Users/m/work").as_deref(), Some("F:\\work"));
        assert_eq!(
            map.map("/Users/m/work/special/x").as_deref(),
            Some("G:\\s\\x")
        );
        assert_eq!(map.map("/Users/m/workshop"), None);
        let back = PathMap::parse(&["F:\\work\\=/home/m/work/".into()]).unwrap();
        assert_eq!(
            back.map("F:\\work\\app").as_deref(),
            Some("/home/m/work/app")
        );
        assert!(PathMap::parse(&["nothing".into()]).is_err());
        assert!(PathMap::parse(&["=x".into()]).is_err());
    }
}
