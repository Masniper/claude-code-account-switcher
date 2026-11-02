//! `claude-acc backup create | inspect | restore | rollback`: the terminal
//! side of [`claude_acc::backup`]. Everything printed here is a path, a
//! count, an account name or a field name: never a file's contents.

use crate::config::{AppConfig, validate_name};
use crate::i18n::{I18n, Msg};
use crate::identity;
use crate::sessions;
use age::secrecy::SecretString;
use claude_acc::backup::{
    self, Action, Error, Manifest, Note, PathMap, Reason, RestoreRequest, Scope, Source,
    SwitchState, Target, Why,
};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

fn target(config: &AppConfig) -> Option<Target> {
    let standard_dir = identity::standard_token_dir()?;
    let standard_json = identity::local_identity_path(&standard_dir)?;
    Some(Target {
        switch: AppConfig {
            base_dir: config.base_dir.clone(),
        },
        standard_dir,
        standard_json,
    })
}

// ---------------------------------------------------------------------------
// create

/// Which accounts go in whole, given the flags. No flags at all means every
/// account; `--project` alone means none whole, only those projects'
/// sessions from each.
fn whole_accounts(
    all: &[Source],
    named: &[String],
    all_flag: bool,
    any_project: bool,
) -> Result<Vec<Source>, String> {
    if all_flag || (named.is_empty() && !any_project) {
        return Ok(all.to_vec());
    }
    named
        .iter()
        .map(|n| {
            all.iter()
                .find(|s| &s.label == n)
                .cloned()
                .ok_or_else(|| n.clone())
        })
        .collect()
}

pub fn create(
    config: &AppConfig,
    i18n: &I18n,
    file: &Path,
    accounts: &[String],
    all_accounts: bool,
    projects: &[PathBuf],
    dry_run: bool,
) -> i32 {
    let Some(t) = target(config) else {
        return fail(i18n, Error::NothingSelected);
    };
    let Some(home) = dirs::home_dir() else {
        return fail(i18n, Error::NothingSelected);
    };
    let all = backup::discover(config, &t.standard_dir, &t.standard_json);
    let whole = match whole_accounts(&all, accounts, all_accounts, !projects.is_empty()) {
        Ok(w) => w,
        Err(name) => return fail(i18n, Error::UnknownAccount(name)),
    };
    let projects: Vec<PathBuf> = projects
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()))
        .collect();
    if whole.is_empty() && projects.is_empty() {
        return fail(i18n, Error::NothingSelected);
    }
    if !dry_run && fs::symlink_metadata(file).is_ok() {
        i18n.print(Msg::BackupFileExists(file.display().to_string()));
        return 1;
    }
    let labels: Vec<String> = whole.iter().map(|s| s.label.clone()).collect();
    let switch = (!whole.is_empty() || !projects.is_empty()).then(|| {
        let links = kept_links(config.all_links().unwrap_or_default(), &labels, &projects);
        let default = config
            .get_default()
            .ok()
            .flatten()
            .filter(|d| labels.contains(d));
        let lock = labels
            .iter()
            .any(|l| l == sessions::DEFAULT_LABEL)
            .then(|| identity::default_lock_path(&config.base_dir));
        (SwitchState { default, links }, lock)
    });
    let scope = Scope {
        home,
        session_accounts: if projects.is_empty() {
            Vec::new()
        } else {
            all.clone()
        },
        accounts: whole,
        projects,
        switch,
    };
    let survey = match backup::survey(&scope) {
        Ok(s) => s,
        Err(e) => return fail(i18n, e),
    };

    i18n.print(Msg::BackupAccounts(join_or_dash(&labels)));
    let roots: Vec<String> = scope
        .projects
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    i18n.print(Msg::BackupProjects(join_or_dash(&roots)));
    if !scope.projects.is_empty() && scope.accounts.is_empty() {
        i18n.print(Msg::BackupProjectsOnly);
    }
    i18n.print(Msg::BackupSize(
        survey.files,
        sessions::human_size(survey.bytes),
    ));
    for root in &survey.empty_projects {
        i18n.print(Msg::BackupEmptyProject(root.clone()));
    }
    print_excluded(i18n, &survey.excluded);
    if dry_run {
        i18n.print(Msg::BackupDryRun);
        return 0;
    }

    let Some(passphrase) = read_passphrase(i18n, true) else {
        return 1;
    };
    match backup::create(&scope, survey, passphrase, file, None) {
        Ok(manifest) => {
            print_redactions(i18n, &manifest);
            i18n.print(Msg::BackupWritten(file.display().to_string()));
            0
        }
        Err(e) => fail(i18n, e),
    }
}

/// The bindings a backup carries: those of the accounts taken whole, and
/// those of folders inside a project taken, whichever account they name.
fn kept_links(
    links: Vec<(String, String)>,
    whole: &[String],
    projects: &[PathBuf],
) -> Vec<(String, String)> {
    links
        .into_iter()
        .filter(|(dir, acc)| {
            whole.contains(acc) || projects.iter().any(|p| Path::new(dir).starts_with(p))
        })
        .collect()
}

fn join_or_dash(items: &[impl AsRef<str>]) -> String {
    if items.is_empty() {
        "-".into()
    } else {
        items
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn print_excluded(i18n: &I18n, excluded: &[backup::Excluded]) {
    if excluded.is_empty() {
        return;
    }
    i18n.print(Msg::BackupLeftOut);
    for e in excluded {
        let why = i18n.msg(match e.reason {
            Reason::Credential => Msg::ReasonCredential,
            Reason::Runtime => Msg::ReasonRuntime,
            Reason::Reinstall => Msg::ReasonReinstall,
            Reason::Special => Msg::ReasonSpecial,
            Reason::Checkout => Msg::ReasonCheckout,
        });
        println!("  {why:<12} {}", e.path);
    }
}

fn print_redactions(i18n: &I18n, manifest: &Manifest) {
    if manifest.redactions.is_empty() {
        return;
    }
    i18n.print(Msg::BackupRedacted);
    for r in &manifest.redactions {
        println!("  {}: {}", r.path, r.field);
    }
}

// ---------------------------------------------------------------------------
// Passphrase and staging

/// From the terminal without echo, or one line of stdin when it is not a
/// terminal (for scripts). Never from an argument.
fn read_passphrase(i18n: &I18n, confirm: bool) -> Option<SecretString> {
    let first = if io::stdin().is_terminal() {
        let first = rpassword::prompt_password(i18n.msg(Msg::BackupPassphrase)).ok()?;
        if confirm {
            let again = rpassword::prompt_password(i18n.msg(Msg::BackupPassphraseAgain)).ok()?;
            if again != first {
                i18n.print(Msg::BackupPassphraseMismatch);
                return None;
            }
        }
        first
    } else {
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok()?;
        line.trim_end_matches(['\r', '\n']).to_string()
    };
    if first.is_empty() {
        i18n.print(Msg::BackupPassphraseEmpty);
        return None;
    }
    Some(SecretString::from(first))
}

/// Where an archive is decrypted to, inside our own state folder and
/// private to the user; removed again however the command ends.
struct Staging(PathBuf);

impl Staging {
    fn new(config: &AppConfig) -> io::Result<Self> {
        let dir = config
            .base_dir
            .join(format!("restore-staging-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // On Windows the user's profile folder is already private to them.
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut b = fs::DirBuilder::new();
            b.mode(0o700);
            b
        };
        builder.create(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn open(config: &AppConfig, i18n: &I18n, file: &Path) -> Result<(Staging, Manifest), i32> {
    let staging = Staging::new(config).map_err(|e| fail(i18n, Error::Io(e)))?;
    let passphrase = read_passphrase(i18n, false).ok_or(1)?;
    let manifest = backup::open(file, passphrase, &staging.0).map_err(|e| fail(i18n, e))?;
    Ok((staging, manifest))
}

// ---------------------------------------------------------------------------
// inspect

pub fn inspect(config: &AppConfig, i18n: &I18n, file: &Path) -> i32 {
    let (_staging, m) = match open(config, i18n, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    i18n.print(Msg::BackupVerified(m.files.len()));
    print_origin(i18n, &m);
    for a in &m.accounts {
        let n = m
            .files
            .iter()
            .filter(|f| f.path.starts_with(&format!("accounts/{}/", a.label)))
            .count();
        i18n.print(Msg::BackupInspectAccount(
            a.label.clone(),
            a.source_dir.clone(),
            n,
            a.projects_only,
        ));
    }
    for p in &m.projects {
        let n = m
            .files
            .iter()
            .filter(|f| f.path.starts_with(&format!("projects/{}/", p.id)))
            .count();
        i18n.print(Msg::BackupInspectProject(p.source_root.clone(), n));
    }
    if let Some(sw) = &m.switch {
        for (dir, acc) in &sw.links {
            i18n.print(Msg::BackupInspectLink(dir.clone(), acc.clone()));
        }
        if let Some(d) = &sw.default {
            i18n.print(Msg::BackupInspectDefault(d.clone()));
        }
    }
    print_redactions(i18n, &m);
    print_excluded(i18n, &m.excluded);
    0
}

fn print_origin(i18n: &I18n, m: &Manifest) {
    let ago = now().saturating_sub(m.created_at);
    i18n.print(Msg::BackupOrigin(
        ago,
        m.source_platform.clone(),
        m.source_home.clone(),
        m.tool_version.clone(),
    ));
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// restore

pub struct RestoreArgs<'a> {
    pub maps: &'a [String],
    pub accounts: &'a [String],
    pub all_accounts: bool,
    pub projects: &'a [String],
    pub reuse: bool,
    pub select: &'a [String],
    pub replace: bool,
    pub dry_run: bool,
}

/// `SRC=DST`, or `SRC` alone.
fn pair(s: &str) -> (String, Option<String>) {
    match s.split_once('=') {
        Some((a, b)) if !b.is_empty() => (a.to_string(), Some(b.to_string())),
        Some((a, _)) => (a.to_string(), None),
        None => (s.to_string(), None),
    }
}

/// The restore the flags ask for, and the backed-up projects left out for
/// want of a destination. The modes:
///
/// - no `--account`/`--project`: every account whole, under its own name,
///   and every project `--map` covers;
/// - `--account` only: those accounts whole, no project files;
/// - `--project` only: those projects, with their sessions and memory from
///   each account, into the target account of the same name;
/// - both, or `--all-accounts` with `--project`: the two together;
/// - `--reuse`: only the definitions of the projects given, nothing else.
fn request(
    m: &Manifest,
    a: &RestoreArgs,
    windows: bool,
) -> Result<(RestoreRequest, Vec<String>), Error> {
    let map = PathMap::parse(a.maps).map_err(Error::BadMap)?;
    let mut accounts = Vec::new();
    for raw in a.accounts {
        let (src, dst) = pair(raw);
        let dst = dst.unwrap_or_else(|| src.clone());
        if dst != sessions::DEFAULT_LABEL && !validate_name(&dst) {
            return Err(Error::UnknownAccount(dst));
        }
        accounts.push((src, dst));
    }
    let no_selection = a.accounts.is_empty() && a.projects.is_empty();
    let whole = a.all_accounts || !a.accounts.is_empty() || no_selection;
    if accounts.is_empty() && !a.reuse {
        accounts = m
            .accounts
            .iter()
            .map(|e| (e.label.clone(), e.label.clone()))
            .collect();
    }
    let mut projects = Vec::new();
    let mut left_out = Vec::new();
    for raw in a.projects {
        let (src, dst) = pair(raw);
        let src = m
            .projects
            .iter()
            .find(|p| p.source_root == src || Path::new(&p.source_root) == Path::new(&src))
            .map(|p| p.source_root.clone())
            .ok_or_else(|| Error::UnmappedProject(src.clone()))?;
        let dst = dst
            .or_else(|| map.map(&src))
            .ok_or_else(|| Error::UnmappedProject(src.clone()))?;
        projects.push((src, PathBuf::from(dst)));
    }
    if a.projects.is_empty() && (no_selection || a.all_accounts) {
        for p in &m.projects {
            match map.map(&p.source_root) {
                Some(dst) => projects.push((p.source_root.clone(), PathBuf::from(dst))),
                None => left_out.push(p.source_root.clone()),
            }
        }
    }
    if a.reuse && projects.is_empty() {
        return Err(Error::NothingSelected);
    }
    Ok((
        RestoreRequest {
            map,
            accounts: if a.reuse { Vec::new() } else { accounts },
            projects,
            whole_accounts: whole && !a.reuse,
            reuse: a.reuse,
            select: a.select.to_vec(),
            replace: a.replace,
            windows_target: windows,
        },
        left_out,
    ))
}

pub fn restore(config: &AppConfig, i18n: &I18n, file: &Path, args: &RestoreArgs) -> i32 {
    let Some(t) = target(config) else {
        return fail(i18n, Error::NothingSelected);
    };
    if let Some(run) = backup::pending(&t) {
        return fail(i18n, Error::JournalPending(run));
    }
    let (staging, m) = match open(config, i18n, file) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let (req, left_out) = match request(&m, args, cfg!(windows)) {
        Ok(r) => r,
        Err(e) => return fail(i18n, e),
    };
    let plan = match backup::plan(&m, &staging.0, &req, &t) {
        Ok(p) => p,
        Err(e) => return fail(i18n, e),
    };

    print_origin(i18n, &m);
    for (src, dst) in &req.accounts {
        let new = plan.new_accounts.contains(dst);
        i18n.print(Msg::RestoreAccount(
            src.clone(),
            dst.clone(),
            new,
            req.whole_accounts,
        ));
    }
    for (src, dst) in &req.projects {
        i18n.print(Msg::RestoreProject(src.clone(), dst.display().to_string()));
    }
    for root in &left_out {
        i18n.print(Msg::RestoreProjectLeftOut(root.clone()));
    }
    i18n.print(Msg::RestoreCounts(
        plan.count(&Action::Create),
        plan.count(&Action::Same),
        plan.count(&Action::Conflict),
        plan.incompatible().count(),
    ));
    if args.dry_run {
        for op in &plan.ops {
            match &op.action {
                Action::Create => println!("  + {}", op.dest.display()),
                Action::Conflict => println!("  ! {}", op.dest.display()),
                _ => {}
            }
        }
    } else {
        let conflicts: Vec<_> = plan
            .ops
            .iter()
            .filter(|o| o.action == Action::Conflict)
            .collect();
        if !conflicts.is_empty() {
            i18n.print(Msg::RestoreConflicts(args.replace));
            for op in conflicts {
                println!("  ! {}", op.dest.display());
            }
        }
    }
    let incompatible: Vec<_> = plan.incompatible().collect();
    if !incompatible.is_empty() {
        i18n.print(Msg::RestoreIncompatible);
        for op in incompatible {
            let Action::Incompatible(why) = &op.action else {
                continue;
            };
            println!("  x {}: {}", op.archive, why_text(i18n, why));
        }
    }
    for l in &plan.links {
        i18n.print(Msg::RestoreLink(
            l.dir.clone(),
            l.account.clone(),
            action_text(i18n, &l.action),
        ));
    }
    if let Some((name, action)) = &plan.default {
        i18n.print(Msg::RestoreDefault(name.clone(), action_text(i18n, action)));
    }
    print_notes(i18n, &plan.notes);

    if args.dry_run {
        i18n.print(Msg::RestoreDryRun);
        return 0;
    }
    match backup::apply(&plan, &m, &t, args.replace, None) {
        Ok(done) => {
            i18n.print(Msg::RestoreDone(
                done.written,
                done.replaced,
                done.skipped,
                done.switch_changes,
            ));
            if done.replaced > 0
                && let Some(run) = &done.run_dir
            {
                i18n.print(Msg::RestoreOriginalsKept(
                    run.join("originals").display().to_string(),
                ));
            }
            i18n.print(Msg::RestoreResumeNote);
            0
        }
        Err(e) => {
            i18n.print(Msg::RestoreUndone);
            fail(i18n, e)
        }
    }
}

fn action_text(i18n: &I18n, a: &Action) -> String {
    i18n.msg(match a {
        Action::Create => Msg::RestoreActionNew,
        Action::Same => Msg::RestoreActionSame,
        _ => Msg::RestoreActionConflict,
    })
}

fn why_text(i18n: &I18n, why: &Why) -> String {
    i18n.msg(match why {
        Why::SymlinkOnWindows => Msg::WhySymlinkOnWindows,
        Why::LinkLeavesFolder => Msg::WhyLinkLeavesFolder,
        Why::Name(backup::NameProblem::Character) => Msg::WhyNameCharacter,
        Why::Name(backup::NameProblem::TrailingDotOrSpace) => Msg::WhyNameTrailing,
        Why::Name(backup::NameProblem::Reserved) => Msg::WhyNameReserved,
        Why::SameNameOtherCase(other) => Msg::WhySameNameOtherCase(other.clone()),
        Why::FolderInTheWay => Msg::WhyFolderInTheWay,
        Why::LinkInTheWay => Msg::WhyLinkInTheWay,
        Why::NotALinkThere => Msg::WhyNotALinkThere,
    })
}

fn print_notes(i18n: &I18n, notes: &[Note]) {
    let mut login = Vec::new();
    for n in notes {
        match n {
            Note::ProjectDirRenamed { account, from, to } => i18n.print(Msg::RestoreDirRenamed(
                account.clone(),
                from.clone(),
                to.clone(),
            )),
            Note::ProjectDirUnmapped { account, name } => {
                i18n.print(Msg::RestoreDirUnmapped(account.clone(), name.clone()))
            }
            Note::ClaudeJsonPathsMapped { account, keys } => {
                i18n.print(Msg::RestoreJsonMapped(account.clone(), *keys))
            }
            Note::NeedsReview { path, field } => {
                i18n.print(Msg::RestoreReview(path.clone(), field.clone()))
            }
            Note::LinkUnmapped { dir } => i18n.print(Msg::RestoreLinkUnmapped(dir.clone())),
            Note::Reinstall { path } => i18n.print(Msg::RestoreReinstall(path.clone())),
            Note::LoginNeeded { account } => login.push(account.clone()),
        }
    }
    if !login.is_empty() {
        i18n.print(Msg::RestoreLogin(login.join(", ")));
    }
}

// ---------------------------------------------------------------------------
// rollback

pub fn rollback(config: &AppConfig, i18n: &I18n) -> i32 {
    let Some(t) = target(config) else {
        return fail(i18n, Error::NothingSelected);
    };
    match backup::rollback(&t) {
        Ok(Some(run)) => {
            i18n.print(Msg::RestoreRolledBack(run.display().to_string()));
            0
        }
        Ok(None) => {
            i18n.print(Msg::RestoreNothingPending);
            0
        }
        Err(e) => fail(i18n, e),
    }
}

fn fail(i18n: &I18n, e: Error) -> i32 {
    let msg = match e {
        // An OS message has no translation; the prefix says where it came from.
        Error::Io(e) => {
            eprintln!("backup: {e}");
            return 1;
        }
        Error::SourceChanged(p) => Msg::BackupErrChanged(p.display().to_string()),
        Error::NothingSelected => Msg::BackupErrNothing,
        Error::UnknownAccount(a) => Msg::LoginNotFound(a),
        Error::WrongPassphrase => Msg::BackupErrPassphrase,
        Error::Corrupt(detail) => Msg::BackupErrCorrupt(detail),
        Error::UnsupportedFormat(v) => Msg::BackupErrFormat(v),
        Error::UnsafePath(p) => Msg::BackupErrUnsafe(p),
        Error::ChecksumMismatch(p) => Msg::BackupErrChecksum(p),
        Error::JournalPending(p) => Msg::BackupErrPending(p.display().to_string()),
        Error::AccountExists(a) => Msg::BackupErrAccountExists(a),
        Error::UnmappedProject(p) => Msg::BackupErrUnmapped(p),
        Error::DuplicateTarget(a) => Msg::BackupErrDuplicate(a),
        Error::BadMap(m) => Msg::BackupErrBadMap(m),
    };
    eprintln!("{}", i18n.msg(msg));
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_acc::backup::{AccountEntry, ProjectEntry};

    fn manifest() -> Manifest {
        let account = |label: &str| AccountEntry {
            label: label.into(),
            source_dir: String::new(),
            projects_only: false,
        };
        Manifest {
            format: backup::FORMAT_VERSION,
            tool_version: String::new(),
            created_at: 0,
            source_platform: "macos".into(),
            source_home: "/Users/m".into(),
            accounts: vec![account("default"), account("work")],
            switch: None,
            projects: vec![
                ProjectEntry {
                    id: "p0".into(),
                    source_root: "/Users/m/work/app".into(),
                },
                ProjectEntry {
                    id: "p1".into(),
                    source_root: "/opt/other".into(),
                },
            ],
            project_dirs: vec![],
            files: vec![],
            redactions: vec![],
            excluded: vec![],
        }
    }

    fn args<'a>(
        maps: &'a [String],
        accounts: &'a [String],
        projects: &'a [String],
    ) -> RestoreArgs<'a> {
        RestoreArgs {
            maps,
            accounts,
            all_accounts: false,
            projects,
            reuse: false,
            select: &[],
            replace: false,
            dry_run: true,
        }
    }

    #[test]
    fn no_selection_restores_every_account_whole_and_every_mapped_project() {
        let maps = ["/Users/m/work=F:\\work".to_string()];
        let (req, left) = request(&manifest(), &args(&maps, &[], &[]), true).unwrap();
        assert!(req.whole_accounts);
        assert_eq!(
            req.accounts,
            vec![
                ("default".into(), "default".into()),
                ("work".into(), "work".into())
            ]
        );
        assert_eq!(
            req.projects,
            vec![("/Users/m/work/app".into(), PathBuf::from("F:\\work\\app"))]
        );
        assert_eq!(left, vec!["/opt/other".to_string()]);
        assert!(req.windows_target);
    }

    #[test]
    fn accounts_only_takes_no_project_files() {
        let maps = ["/Users/m/work=F:\\work".to_string()];
        let accounts = ["work=work-mac".to_string()];
        let (req, left) = request(&manifest(), &args(&maps, &accounts, &[]), false).unwrap();
        assert!(req.whole_accounts);
        assert_eq!(req.accounts, vec![("work".into(), "work-mac".into())]);
        assert!(req.projects.is_empty() && left.is_empty());
    }

    #[test]
    fn project_only_moves_its_sessions_from_every_account_into_a_chosen_folder() {
        let projects = ["/Users/m/work/app=D:\\new\\app".to_string()];
        let (req, _) = request(&manifest(), &args(&[], &[], &projects), true).unwrap();
        assert!(!req.whole_accounts);
        assert_eq!(req.accounts.len(), 2);
        assert_eq!(
            req.projects,
            vec![("/Users/m/work/app".into(), PathBuf::from("D:\\new\\app"))]
        );
    }

    #[test]
    fn a_project_needs_a_destination() {
        let projects = ["/Users/m/work/app".to_string()];
        assert!(matches!(
            request(&manifest(), &args(&[], &[], &projects), true),
            Err(Error::UnmappedProject(_))
        ));
        let unknown = ["/nowhere=/x".to_string()];
        assert!(matches!(
            request(&manifest(), &args(&[], &[], &unknown), true),
            Err(Error::UnmappedProject(_))
        ));
    }

    #[test]
    fn reuse_takes_no_accounts_and_needs_a_project() {
        let projects = ["/Users/m/work/app=/home/m/other".to_string()];
        let mut a = args(&[], &[], &projects);
        a.reuse = true;
        let (req, _) = request(&manifest(), &a, false).unwrap();
        assert!(req.reuse && req.accounts.is_empty() && !req.whole_accounts);
        let mut a = args(&[], &[], &[]);
        a.reuse = true;
        assert!(matches!(
            request(&manifest(), &a, false),
            Err(Error::NothingSelected)
        ));
    }

    #[test]
    fn a_target_account_name_must_be_valid() {
        let accounts = ["work=../escape".to_string()];
        assert!(matches!(
            request(&manifest(), &args(&[], &accounts, &[]), false),
            Err(Error::UnknownAccount(_))
        ));
    }

    #[test]
    fn a_project_backup_carries_the_bindings_inside_it() {
        let links = vec![
            ("/w/app".to_string(), "ctpo".to_string()),
            ("/w/app/sub".to_string(), "max".to_string()),
            ("/w/application".to_string(), "ctpo".to_string()),
            ("/elsewhere".to_string(), "home".to_string()),
        ];
        let kept = kept_links(links.clone(), &[], &[PathBuf::from("/w/app")]);
        assert_eq!(kept, links[..2].to_vec());
        let kept = kept_links(links.clone(), &["home".to_string()], &[]);
        assert_eq!(kept, vec![links[3].clone()]);
    }

    #[test]
    fn whole_accounts_follow_the_create_flags() {
        let all: Vec<Source> = ["default", "work"]
            .iter()
            .map(|l| Source {
                label: l.to_string(),
                config_dir: PathBuf::new(),
                home_json: None,
            })
            .collect();
        let labels = |v: Vec<Source>| v.into_iter().map(|s| s.label).collect::<Vec<_>>();
        assert_eq!(
            labels(whole_accounts(&all, &[], false, false).unwrap()),
            vec!["default", "work"]
        );
        assert!(whole_accounts(&all, &[], false, true).unwrap().is_empty());
        assert_eq!(
            labels(whole_accounts(&all, &[], true, true).unwrap()),
            vec!["default", "work"]
        );
        assert_eq!(
            labels(whole_accounts(&all, &["work".into()], false, true).unwrap()),
            vec!["work"]
        );
        assert_eq!(
            whole_accounts(&all, &["nope".into()], false, false).unwrap_err(),
            "nope"
        );
    }
}
