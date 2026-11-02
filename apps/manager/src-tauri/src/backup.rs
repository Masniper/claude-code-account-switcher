//! Backup and restore in the window: `claude_acc::backup`, with an opened
//! archive kept here between Open, Preview and Restore, so the passphrase
//! crosses into the backend once and the archive is decrypted once.
//!
//! What reaches the webview is paths, counts, account names and field
//! names, as on the command line; never a file's contents.
use crate::backend::{self, Backend};
use claude_acc::backup::{
    self, Action, Error, Manifest, NameProblem, Note, Reason, RestoreChoice, RestoreRequest,
    SecretString, Staging, Target, Why,
};
use claude_acc::config::AppConfig;
use claude_acc::identity;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager};

/// A failure the window can word: a code, and the one name or path it is
/// about, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub code: &'static str,
    pub subject: Option<String>,
}

impl Failure {
    fn new(code: &'static str, subject: Option<String>) -> Self {
        Self { code, subject }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        let path = |p: PathBuf| Some(p.display().to_string());
        match e {
            // An OS message names a path at most, never a file's contents.
            Error::Io(e) => Failure::new("backup_io", Some(e.to_string())),
            Error::SourceChanged(p) => Failure::new("backup_source_changed", path(p)),
            Error::NothingSelected => Failure::new("backup_nothing_selected", None),
            Error::UnknownAccount(a) => Failure::new("backup_unknown_account", Some(a)),
            Error::WrongPassphrase => Failure::new("backup_wrong_passphrase", None),
            Error::Corrupt(detail) => Failure::new("backup_corrupt", Some(detail)),
            Error::UnsupportedFormat(v) => {
                Failure::new("backup_unsupported_format", Some(v.to_string()))
            }
            Error::UnsafePath(p) => Failure::new("backup_unsafe_path", Some(p)),
            Error::ChecksumMismatch(p) => Failure::new("backup_checksum", Some(p)),
            Error::JournalPending(p) => Failure::new("backup_pending", path(p)),
            Error::AccountExists(a) => Failure::new("backup_account_exists", Some(a)),
            Error::UnmappedProject(p) => Failure::new("backup_unmapped_project", Some(p)),
            Error::DuplicateTarget(a) => Failure::new("backup_duplicate_target", Some(a)),
            Error::BadMap(m) => Failure::new("backup_bad_map", Some(m)),
        }
    }
}

type Outcome<T> = Result<T, Failure>;

/// An archive opened and checked, waiting for a restore.
struct Opened {
    manifest: Manifest,
    // Declared last: dropped after the manifest that names its files.
    staging: Staging,
}

/// Managed by Tauri. One archive open at a time; opening another, or
/// closing, removes the previous staging folder.
#[derive(Default)]
pub struct BackupState(Mutex<Option<Opened>>);

impl BackupState {
    fn lock(&self) -> MutexGuard<'_, Option<Opened>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn target(config: &AppConfig, standard: &Path) -> Outcome<Target> {
    let standard_json = identity::local_identity_path(standard)
        .ok_or_else(|| Failure::new("backup_nothing_selected", None))?;
    Ok(Target {
        switch: AppConfig {
            base_dir: config.base_dir.clone(),
        },
        standard_dir: standard.to_path_buf(),
        standard_json,
    })
}

fn passphrase(raw: String) -> Outcome<SecretString> {
    if raw.is_empty() {
        return Err(Failure::new("backup_empty_passphrase", None));
    }
    Ok(SecretString::from(raw))
}

// ---------------------------------------------------------------------------
// Views

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Left {
    pub path: String,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removed {
    pub path: String,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurveyView {
    /// Accounts taken whole.
    pub accounts: Vec<String>,
    pub projects: Vec<String>,
    /// Projects chosen with no account whole: their sessions come from
    /// every account.
    pub projects_only: bool,
    pub files: usize,
    pub bytes: u64,
    pub empty_projects: Vec<String>,
    pub excluded: Vec<Left>,
}

fn survey_view(scope: &backup::Scope, survey: &backup::Survey) -> SurveyView {
    SurveyView {
        accounts: scope.accounts.iter().map(|a| a.label.clone()).collect(),
        projects: scope
            .projects
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        projects_only: scope.accounts.is_empty() && !scope.projects.is_empty(),
        files: survey.files,
        bytes: survey.bytes,
        empty_projects: survey.empty_projects.clone(),
        excluded: left(&survey.excluded),
    }
}

fn left(excluded: &[backup::Excluded]) -> Vec<Left> {
    excluded
        .iter()
        .map(|e| Left {
            path: e.path.clone(),
            reason: e.reason,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedView {
    pub file: String,
    pub files: usize,
    pub redactions: Vec<Removed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedAccount {
    pub label: String,
    pub source_dir: String,
    pub files: usize,
    pub projects_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedProject {
    pub source_root: String,
    pub files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub dir: String,
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedView {
    pub file: String,
    pub files: usize,
    pub created_at: u64,
    pub source_platform: String,
    pub source_home: String,
    pub tool_version: String,
    pub accounts: Vec<OpenedAccount>,
    pub projects: Vec<OpenedProject>,
    pub bindings: Vec<Binding>,
    pub default_account: Option<String>,
    /// The folder the projects share, for a first mapping row to start
    /// from; the home folder when there are no projects.
    pub suggested_from: String,
    pub redactions: Vec<Removed>,
    pub excluded: Vec<Left>,
}

fn opened_view(file: &Path, m: &Manifest) -> OpenedView {
    let count = |prefix: String| {
        m.files
            .iter()
            .filter(|f| f.path.starts_with(&prefix))
            .count()
    };
    OpenedView {
        file: file.display().to_string(),
        files: m.files.len(),
        created_at: m.created_at,
        source_platform: m.source_platform.clone(),
        source_home: m.source_home.clone(),
        tool_version: m.tool_version.clone(),
        accounts: m
            .accounts
            .iter()
            .map(|a| OpenedAccount {
                label: a.label.clone(),
                source_dir: a.source_dir.clone(),
                files: count(format!("accounts/{}/", a.label)),
                projects_only: a.projects_only,
            })
            .collect(),
        projects: m
            .projects
            .iter()
            .map(|p| OpenedProject {
                source_root: p.source_root.clone(),
                files: count(format!("projects/{}/", p.id)),
            })
            .collect(),
        bindings: m
            .switch
            .iter()
            .flat_map(|s| &s.links)
            .map(|(dir, account)| Binding {
                dir: dir.clone(),
                account: account.clone(),
            })
            .collect(),
        default_account: m.switch.as_ref().and_then(|s| s.default.clone()),
        suggested_from: common_parent(m.projects.iter().map(|p| p.source_root.as_str()))
            .unwrap_or_else(|| m.source_home.clone()),
        redactions: m
            .redactions
            .iter()
            .map(|r| Removed {
                path: r.path.clone(),
                field: r.field.clone(),
            })
            .collect(),
        excluded: left(&m.excluded),
    }
}

/// The longest folder every path is inside, by whole components, written
/// as the source wrote it. `None` for no paths, or when they share only the
/// root. One path's parent is its folder.
fn common_parent<'a>(paths: impl Iterator<Item = &'a str>) -> Option<String> {
    let split = |p: &'a str| -> (char, Vec<&'a str>) {
        let sep = if p.contains('\\') { '\\' } else { '/' };
        (sep, p.split(sep).collect())
    };
    let mut paths = paths.map(split);
    let (sep, mut common) = paths.next()?;
    // A single project's root is the project, not its folder.
    common.pop();
    for (_, parts) in paths {
        let same = common
            .iter()
            .zip(&parts[..parts.len().saturating_sub(1)])
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(same);
    }
    // `["", "Users"]` is `/Users`; `[""]` alone is just the root.
    (common.iter().filter(|c| !c.is_empty()).count() > 0).then(|| common.join(&sep.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountMove {
    pub from: String,
    pub to: String,
    /// Created by this restore.
    pub new: bool,
    /// Restored whole, not only the restored projects' sessions.
    pub whole: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectMove {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Create,
    Same,
    Conflict,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileStep {
    pub archive: String,
    pub dest: String,
    pub step: Step,
    /// Why an incompatible file cannot be written, as a code.
    pub why: Option<&'static str>,
    /// The other file, for a name that differs only by case.
    pub other: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkStep {
    pub dir: String,
    pub account: String,
    pub step: Step,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteView {
    ProjectDirRenamed {
        account: String,
        from: String,
        to: String,
    },
    ProjectDirUnmapped {
        account: String,
        name: String,
    },
    ClaudeJsonPathsMapped {
        account: String,
        keys: usize,
    },
    NeedsReview {
        path: String,
        field: String,
    },
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub create: usize,
    pub same: usize,
    pub conflict: usize,
    pub incompatible: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanView {
    pub accounts: Vec<AccountMove>,
    pub projects: Vec<ProjectMove>,
    /// Backed-up projects no mapping or destination covered.
    pub left_out: Vec<String>,
    pub counts: Counts,
    pub files: Vec<FileStep>,
    pub links: Vec<LinkStep>,
    pub default_account: Option<LinkStep>,
    pub notes: Vec<NoteView>,
    pub replace: bool,
}

fn step(a: &Action) -> Step {
    match a {
        Action::Create => Step::Create,
        Action::Same => Step::Same,
        Action::Conflict => Step::Conflict,
        Action::Incompatible(_) => Step::Incompatible,
    }
}

fn why(a: &Action) -> (Option<&'static str>, Option<String>) {
    let Action::Incompatible(w) = a else {
        return (None, None);
    };
    let code = match w {
        Why::SymlinkOnWindows => "symlink_on_windows",
        Why::LinkLeavesFolder => "link_leaves_folder",
        Why::Name(NameProblem::Character) => "name_character",
        Why::Name(NameProblem::TrailingDotOrSpace) => "name_trailing",
        Why::Name(NameProblem::Reserved) => "name_reserved",
        Why::SameNameOtherCase(other) => {
            return (Some("same_name_other_case"), Some(other.clone()));
        }
        Why::FolderInTheWay => "folder_in_the_way",
        Why::LinkInTheWay => "link_in_the_way",
        Why::NotALinkThere => "not_a_link_there",
    };
    (Some(code), None)
}

fn plan_view(req: &RestoreRequest, left_out: Vec<String>, plan: &backup::Plan) -> PlanView {
    PlanView {
        accounts: req
            .accounts
            .iter()
            .map(|(from, to)| AccountMove {
                from: from.clone(),
                to: to.clone(),
                new: plan.new_accounts.contains(to),
                whole: req.whole_accounts,
            })
            .collect(),
        projects: req
            .projects
            .iter()
            .map(|(from, to)| ProjectMove {
                from: from.clone(),
                to: to.display().to_string(),
            })
            .collect(),
        left_out,
        counts: Counts {
            create: plan.count(&Action::Create),
            same: plan.count(&Action::Same),
            conflict: plan.count(&Action::Conflict),
            incompatible: plan.incompatible().count(),
        },
        files: plan
            .ops
            .iter()
            .map(|op| {
                let (why, other) = why(&op.action);
                FileStep {
                    archive: op.archive.clone(),
                    dest: op.dest.display().to_string(),
                    step: step(&op.action),
                    why,
                    other,
                }
            })
            .collect(),
        links: plan
            .links
            .iter()
            .map(|l| LinkStep {
                dir: l.dir.clone(),
                account: l.account.clone(),
                step: step(&l.action),
            })
            .collect(),
        default_account: plan.default.as_ref().map(|(account, action)| LinkStep {
            dir: String::new(),
            account: account.clone(),
            step: step(action),
        }),
        notes: plan.notes.iter().map(note_view).collect(),
        replace: req.replace,
    }
}

fn note_view(n: &Note) -> NoteView {
    match n.clone() {
        Note::ProjectDirRenamed { account, from, to } => {
            NoteView::ProjectDirRenamed { account, from, to }
        }
        Note::ProjectDirUnmapped { account, name } => {
            NoteView::ProjectDirUnmapped { account, name }
        }
        Note::ClaudeJsonPathsMapped { account, keys } => {
            NoteView::ClaudeJsonPathsMapped { account, keys }
        }
        Note::NeedsReview { path, field } => NoteView::NeedsReview { path, field },
        Note::LinkUnmapped { dir } => NoteView::LinkUnmapped { dir },
        Note::LoginNeeded { account } => NoteView::LoginNeeded { account },
        Note::Reinstall { path } => NoteView::Reinstall { path },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedView {
    pub written: usize,
    pub replaced: usize,
    pub skipped: usize,
    pub switch_changes: usize,
    /// Where the originals of replaced files were kept.
    pub originals: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Outcome<T> + Send + 'static,
) -> Outcome<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|_| Failure::new("backup_failed", None))?
}

fn scope_of(
    app: &AppHandle,
    accounts: &[String],
    all_accounts: bool,
    projects: &[String],
) -> Outcome<backup::Scope> {
    let b = app.state::<Backend>();
    let t = target(&b.config, &b.standard)?;
    let home = b
        .home()
        .ok_or_else(|| Failure::new("backup_nothing_selected", None))?
        .to_path_buf();
    let projects: Vec<PathBuf> = projects.iter().map(PathBuf::from).collect();
    Ok(backup::scope(
        &b.config,
        &t.standard_dir,
        &t.standard_json,
        home,
        accounts,
        all_accounts,
        &projects,
    )?)
}

/// What a backup of this choice would hold. Reads no file's contents.
#[tauri::command]
pub async fn backup_survey(
    app: AppHandle,
    accounts: Vec<String>,
    all_accounts: bool,
    projects: Vec<String>,
) -> Outcome<SurveyView> {
    blocking(app, move |app| {
        let scope = scope_of(app, &accounts, all_accounts, &projects)?;
        let survey = backup::survey(&scope)?;
        Ok(survey_view(&scope, &survey))
    })
    .await
}

#[tauri::command]
pub async fn backup_create(
    app: AppHandle,
    file: String,
    accounts: Vec<String>,
    all_accounts: bool,
    projects: Vec<String>,
    passphrase: String,
) -> Outcome<CreatedView> {
    blocking(app, move |app| {
        let secret = self::passphrase(passphrase)?;
        let out = PathBuf::from(&file);
        if std::fs::symlink_metadata(&out).is_ok() {
            return Err(Failure::new("backup_file_exists", Some(file)));
        }
        let scope = scope_of(app, &accounts, all_accounts, &projects)?;
        let survey = backup::survey(&scope)?;
        let manifest = backup::create(&scope, survey, secret, &out, None)?;
        Ok(CreatedView {
            file,
            files: manifest.files.len(),
            redactions: manifest
                .redactions
                .iter()
                .map(|r| Removed {
                    path: r.path.clone(),
                    field: r.field.clone(),
                })
                .collect(),
        })
    })
    .await
}

/// Decrypt and check an archive, and keep it open for a restore.
#[tauri::command]
pub async fn backup_open(app: AppHandle, file: String, passphrase: String) -> Outcome<OpenedView> {
    blocking(app, move |app| {
        let secret = self::passphrase(passphrase)?;
        let state = app.state::<BackupState>();
        // Close any previous one first: its staging folder goes now.
        state.lock().take();
        let base = &app.state::<Backend>().config.base_dir;
        let staging = Staging::new(base).map_err(Error::Io)?;
        let path = PathBuf::from(&file);
        let manifest = backup::open(&path, secret, staging.path())?;
        let view = opened_view(&path, &manifest);
        *state.lock() = Some(Opened { manifest, staging });
        Ok(view)
    })
    .await
}

fn planned<T>(
    app: &AppHandle,
    choice: &RestoreChoice,
    then: impl FnOnce(&Opened, &Target, RestoreRequest, Vec<String>, backup::Plan) -> Outcome<T>,
) -> Outcome<T> {
    let b = app.state::<Backend>();
    let t = target(&b.config, &b.standard)?;
    let state = app.state::<BackupState>();
    let guard = state.lock();
    let opened = guard
        .as_ref()
        .ok_or_else(|| Failure::new("backup_not_open", None))?;
    let (req, left_out) = backup::restore_request(&opened.manifest, choice, cfg!(windows))?;
    let plan = backup::plan(&opened.manifest, opened.staging.path(), &req, &t)?;
    then(opened, &t, req, left_out, plan)
}

/// Every file a restore would write, and every conflict; writes nothing.
#[tauri::command]
pub async fn backup_preview(app: AppHandle, choice: RestoreChoice) -> Outcome<PlanView> {
    blocking(app, move |app| {
        planned(app, &choice, |_, _, req, left_out, plan| {
            Ok(plan_view(&req, left_out, &plan))
        })
    })
    .await
}

/// Plan again, from what is on disk now, and apply. The archive stays open,
/// so a second restore of the same backup can be previewed at once.
#[tauri::command]
pub async fn backup_restore(app: AppHandle, choice: RestoreChoice) -> Outcome<AppliedView> {
    blocking(app, move |app| {
        let done = planned(app, &choice, |opened, t, req, _, plan| {
            Ok(backup::apply(
                &plan,
                &opened.manifest,
                t,
                req.replace,
                None,
            )?)
        })?;
        // New accounts and bindings show on the other pages at once.
        let _ = backend::refresh(app, false);
        Ok(AppliedView {
            written: done.written,
            replaced: done.replaced,
            skipped: done.skipped,
            switch_changes: done.switch_changes,
            originals: (done.replaced > 0)
                .then(|| {
                    done.run_dir
                        .map(|r| r.join("originals").display().to_string())
                })
                .flatten(),
        })
    })
    .await
}

#[tauri::command]
pub async fn backup_close(app: AppHandle) -> Outcome<()> {
    blocking(app, |app| {
        app.state::<BackupState>().lock().take();
        Ok(())
    })
    .await
}

/// The folder of a restore that neither finished nor was undone.
#[tauri::command]
pub async fn backup_pending(app: AppHandle) -> Outcome<Option<String>> {
    blocking(app, |app| {
        let b = app.state::<Backend>();
        let t = target(&b.config, &b.standard)?;
        Ok(backup::pending(&t).map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn backup_rollback(app: AppHandle) -> Outcome<Option<String>> {
    blocking(app, |app| {
        let b = app.state::<Backend>();
        let t = target(&b.config, &b.standard)?;
        let undone = backup::rollback(&t)?;
        let _ = backend::refresh(app, false);
        Ok(undone.map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn choose_backup_save() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .add_filter("Claude Accounts backup", &["ccbackup"])
        .set_file_name("claude-backup.ccbackup")
        .save_file()
        .await
        .map(|p| p.path().to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn choose_backup_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .add_filter("Claude Accounts backup", &["ccbackup"])
        .pick_file()
        .await
        .map(|p| p.path().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_acc::backup::{AccountEntry, Excluded, FileEntry, ProjectEntry, SwitchState};

    fn manifest() -> Manifest {
        Manifest {
            format: backup::FORMAT_VERSION,
            tool_version: "0.21.0".into(),
            created_at: 10,
            source_platform: "macos".into(),
            source_home: "/Users/m".into(),
            accounts: vec![AccountEntry {
                label: "work".into(),
                source_dir: "/Users/m/.claude-switch/accounts/work".into(),
                projects_only: true,
            }],
            switch: Some(SwitchState {
                default: Some("work".into()),
                links: vec![("/Users/m/work/app".into(), "work".into())],
            }),
            projects: vec![
                ProjectEntry {
                    id: "p0".into(),
                    source_root: "/Users/m/work/app".into(),
                },
                ProjectEntry {
                    id: "p1".into(),
                    source_root: "/Users/m/work/site".into(),
                },
            ],
            project_dirs: vec![],
            files: [
                "accounts/work/config/projects/x/s.jsonl",
                "projects/p0/CLAUDE.md",
            ]
            .iter()
            .map(|p| FileEntry {
                path: p.to_string(),
                size: 1,
                sha256: String::new(),
                link: None,
            })
            .collect(),
            redactions: vec![backup::Redaction {
                path: "accounts/work/config/settings.json".into(),
                field: "env.API_TOKEN".into(),
            }],
            excluded: vec![Excluded {
                path: "accounts/work/config/.credentials.json".into(),
                reason: Reason::Credential,
            }],
        }
    }

    #[test]
    fn an_opened_backup_is_described_by_names_and_counts() {
        let view = opened_view(Path::new("/b/env.ccbackup"), &manifest());
        assert_eq!(view.files, 2);
        assert_eq!(view.accounts[0].files, 1);
        assert!(view.accounts[0].projects_only);
        assert_eq!(view.projects[0].files, 1);
        assert_eq!(view.projects[1].files, 0);
        assert_eq!(view.bindings[0].account, "work");
        assert_eq!(view.default_account.as_deref(), Some("work"));
        assert_eq!(view.suggested_from, "/Users/m/work");
        assert_eq!(view.redactions[0].field, "env.API_TOKEN");
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["excluded"][0]["reason"], "credential");
    }

    #[test]
    fn the_suggested_folder_is_the_one_the_projects_share() {
        let parent = |paths: &[&str]| common_parent(paths.iter().copied());
        assert_eq!(
            parent(&["/Users/m/work/app"]).as_deref(),
            Some("/Users/m/work")
        );
        assert_eq!(
            parent(&["/Users/m/work/app", "/Users/m/work/b/c"]).as_deref(),
            Some("/Users/m/work")
        );
        assert_eq!(
            parent(&[r"F:\work\a", r"F:\work\b"]).as_deref(),
            Some(r"F:\work")
        );
        // Only the root in common: no useful suggestion.
        assert_eq!(parent(&["/a/x", "/b/y"]), None);
        assert_eq!(parent(&[]), None);
    }

    #[test]
    fn every_library_error_has_a_code_and_carries_no_more_than_a_name() {
        let cases = [
            (Error::WrongPassphrase, "backup_wrong_passphrase", None),
            (
                Error::AccountExists("work".into()),
                "backup_account_exists",
                Some("work"),
            ),
            (
                Error::ChecksumMismatch("projects/p0/x".into()),
                "backup_checksum",
                Some("projects/p0/x"),
            ),
            (Error::NothingSelected, "backup_nothing_selected", None),
            (Error::BadMap("x".into()), "backup_bad_map", Some("x")),
        ];
        for (error, code, subject) in cases {
            let f = Failure::from(error);
            assert_eq!((f.code, f.subject.as_deref()), (code, subject));
        }
    }

    #[test]
    fn a_plan_is_shown_step_by_step_with_reasons_as_codes() {
        let req = RestoreRequest {
            accounts: vec![("work".into(), "work-mac".into())],
            projects: vec![("/Users/m/work/app".into(), PathBuf::from(r"F:\work\app"))],
            whole_accounts: false,
            ..RestoreRequest::default()
        };
        let plan = backup::Plan {
            new_accounts: vec!["work-mac".into()],
            notes: vec![Note::LoginNeeded {
                account: "work-mac".into(),
            }],
            default: Some(("work-mac".into(), Action::Conflict)),
            ..backup::Plan::default()
        };
        let view = plan_view(&req, vec!["/Users/m/work/site".into()], &plan);
        assert_eq!(
            view.accounts,
            vec![AccountMove {
                from: "work".into(),
                to: "work-mac".into(),
                new: true,
                whole: false
            }]
        );
        assert_eq!(view.projects[0].to, r"F:\work\app");
        assert_eq!(view.left_out, vec!["/Users/m/work/site".to_string()]);
        assert_eq!(view.default_account.unwrap().step, Step::Conflict);
        let json = serde_json::to_value(&view.notes).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{ "kind": "login_needed", "account": "work-mac" }])
        );
        assert_eq!(
            why(&Action::Incompatible(Why::SameNameOtherCase("a/B".into()))),
            (Some("same_name_other_case"), Some("a/B".into()))
        );
        assert_eq!(why(&Action::Create), (None, None));
    }

    #[test]
    fn an_empty_passphrase_is_refused_before_anything_is_read() {
        assert_eq!(
            passphrase(String::new()).unwrap_err().code,
            "backup_empty_passphrase"
        );
        assert!(passphrase("x".into()).is_ok());
    }
}
