use std::path::Path;

use crate::config::AppConfig;
use crate::i18n::{I18n, Msg};
use crate::identity::{self, CachedInfo};
use crate::ui;

pub fn run(config: &AppConfig, i18n: &I18n) {
    let default_acc = config.get_default().ok().flatten();
    let accounts = config.list_accounts().unwrap_or_default();

    // Standard ~/.claude/ shows up only if the user has actually logged into
    // the standard config — so empty installs don't get a phantom row.
    let standard = identity::standard_token_dir();
    let standard_logged_in = standard
        .as_deref()
        .and_then(identity::current_token_hash)
        .is_some()
        || identity::read_cache_at(&identity::default_cache_path(&config.base_dir)).is_some();

    if accounts.is_empty() && !standard_logged_in {
        ui::empty_state(
            &i18n.msg(Msg::ListEmpty),
            "Get started by adding your first account",
            "claude-acc add work"
        );
        return;
    }

    ui::header(&i18n.msg(Msg::ListHeader));

    for acc in &accounts {
        let acc_dir = config.account_path(acc);
        let cache = identity::read_cache(&acc_dir);
        let is_default = Some(acc.as_str()) == default_acc.as_deref();
        
        let (email, time_ago, has_drift) = extract_cache_info(cache.as_ref(), Some(acc_dir.as_path()));
        ui::account_item(acc, email, time_ago, is_default, has_drift);
    }

    if standard_logged_in {
        let cache_path = identity::default_cache_path(&config.base_dir);
        let cache = identity::read_cache_at(&cache_path);
        let (email, time_ago, has_drift) = extract_cache_info(cache.as_ref(), standard.as_deref());
        
        print!("    {}~/.claude/{}", ui::colors::DIM, ui::colors::RESET);
        if let Some(e) = email {
            print!("  {}", e);
        }
        if has_drift {
            print!(" {}*{}", ui::colors::YELLOW, ui::colors::RESET);
        }
        if let Some(t) = time_ago {
            print!("  {}{}{}", ui::colors::DIM, t, ui::colors::RESET);
        }
        println!("  {}{}{}", ui::colors::DIM, i18n.msg(Msg::ListStandard), ui::colors::RESET);
    }
    println!();
}

/// Extract (email, time_ago, has_drift) from cache
fn extract_cache_info(
    cache: Option<&CachedInfo>,
    token_dir: Option<&Path>,
) -> (Option<&str>, Option<String>, bool) {
    let Some(cache) = cache else {
        return (None, None, false);
    };
    
    let email = cache.email.as_deref();
    
    let time_ago = cache
        .fetched_at
        .and_then(identity::seconds_since)
        .map(|secs| format_relative_time(secs));
    
    let has_drift = match (
        cache.token_hash.as_deref(),
        token_dir.and_then(identity::current_token_hash),
    ) {
        (Some(cached), Some(current)) if cached != current => true,
        _ => false,
    };
    
    (email, time_ago, has_drift)
}

/// Format seconds into human-readable relative time
fn format_relative_time(secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    
    if secs < MINUTE {
        "just now".to_string()
    } else if secs < HOUR {
        format!("{}m ago", secs / MINUTE)
    } else if secs < DAY {
        format!("{}h ago", secs / HOUR)
    } else {
        format!("{}d ago", secs / DAY)
    }
}

