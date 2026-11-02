//! `claude-acc backup create | inspect | restore | rollback`: the terminal
//! side of [`claude_acc::backup`]. Everything printed here is a path, a
//! count, an account name or a field name: never a file's contents.

use crate::config::AppConfig;
use crate::i18n::{I18n, Msg};
use crate::identity;
use crate::sessions;
use age::secrecy::SecretString;
use claude_acc::backup::{
    self, Action, Error, Manifest, Note, Reason, RestoreChoice, Staging, Target, Why,
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
    let scope = match backup::scope(
        config,
        &t.standard_dir,
        &t.standard_json,
        home,
        accounts,
        all_accounts,
        projects,
    ) {
        Ok(s) => s,
        Err(e) => return fail(i18n, e),
    };
    if !dry_run && fs::symlink_metadata(file).is_ok() {
        i18n.print(Msg::BackupFileExists(file.display().to_string()));
        return 1;
    }
    let labels: Vec<&str> = scope.accounts.iter().map(|s| s.label.as_str()).collect();
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

fn open(config: &AppConfig, i18n: &I18n, file: &Path) -> Result<(Staging, Manifest), i32> {
    let staging = Staging::new(&config.base_dir).map_err(|e| fail(i18n, Error::Io(e)))?;
    let passphrase = read_passphrase(i18n, false).ok_or(1)?;
    let manifest = backup::open(file, passphrase, staging.path()).map_err(|e| fail(i18n, e))?;
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

fn choice(a: &RestoreArgs) -> RestoreChoice {
    RestoreChoice {
        maps: a.maps.to_vec(),
        accounts: a.accounts.to_vec(),
        all_accounts: a.all_accounts,
        projects: a.projects.to_vec(),
        reuse: a.reuse,
        select: a.select.to_vec(),
        replace: a.replace,
    }
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
    let (req, left_out) = match backup::restore_request(&m, &choice(args), cfg!(windows)) {
        Ok(r) => r,
        Err(e) => return fail(i18n, e),
    };
    let plan = match backup::plan(&m, staging.path(), &req, &t) {
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
