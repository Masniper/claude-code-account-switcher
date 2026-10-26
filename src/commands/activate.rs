use crate::config::AppConfig;
use crate::resolve;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum ShellSyntax {
    Posix,
    PowerShell,
}

/// The `CLAUDE_CONFIG_DIR` for `cwd`: the bound or default account's
/// directory, or `None` for the standard account, an unbound directory, or
/// an account whose directory is gone. What `run` prints, and what the
/// Windows VS Code wrapper sets.
pub fn config_dir_for(config: &AppConfig, cwd: &Path) -> Option<PathBuf> {
    match resolve::resolve_account(config, cwd).as_deref() {
        Some("default") | None => None,
        Some(name) => Some(config.account_path(name)).filter(|p| p.is_dir()),
    }
}

pub fn run(config: &AppConfig, shell: ShellSyntax) {
    let cwd = std::env::current_dir().expect("Cannot get current directory");
    let account = resolve::resolve_account(config, &cwd);

    match account.as_deref() {
        Some("default") | None => match shell {
            ShellSyntax::Posix => println!("unset CLAUDE_CONFIG_DIR"),
            ShellSyntax::PowerShell => {
                println!("Remove-Item Env:\\CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue")
            }
        },
        Some(name) => {
            let path = config.account_path(name);
            if path.is_dir() {
                match shell {
                    ShellSyntax::Posix => println!("export CLAUDE_CONFIG_DIR='{}'", path.display()),
                    ShellSyntax::PowerShell => {
                        println!("$env:CLAUDE_CONFIG_DIR = '{}'", path.display())
                    }
                }
            } else {
                match shell {
                    ShellSyntax::Posix => println!("unset CLAUDE_CONFIG_DIR"),
                    ShellSyntax::PowerShell => println!(
                        "Remove-Item Env:\\CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue"
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_bound_directory_gets_its_accounts_dir_and_nothing_else_does() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("cc-activate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let config = AppConfig {
            base_dir: root.join("switch"),
        };
        config.init().unwrap();
        let (bound, gone, free) = (
            root.join("p/work"),
            root.join("p/gone"),
            root.join("p/free"),
        );
        for d in [&bound, &gone, &free] {
            fs::create_dir_all(d).unwrap();
        }
        fs::create_dir_all(config.account_path("work")).unwrap();
        config.set_link(bound.to_str().unwrap(), "work").unwrap();
        config.set_link(gone.to_str().unwrap(), "deleted").unwrap();

        assert_eq!(
            config_dir_for(&config, &bound),
            Some(config.account_path("work"))
        );
        assert_eq!(
            config_dir_for(&config, &bound.join("sub")),
            Some(config.account_path("work"))
        );
        assert_eq!(config_dir_for(&config, &gone), None);
        assert_eq!(config_dir_for(&config, &free), None);
        fs::remove_dir_all(root).unwrap();
    }
}
