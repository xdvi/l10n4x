//! TMS plugin contract validation (`l10n4x plugin validate`).

use super::{in_process_plugin, plugin_binary_name, CORE_PROVIDERS, KNOWN_PLUGINS};
use crate::config::Config;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Debug, Clone)]
struct Check {
    label: &'static str,
    status: CheckStatus,
    detail: String,
}

#[derive(Debug, Clone)]
struct PluginReport {
    id: String,
    checks: Vec<Check>,
}

impl PluginReport {
    fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }

    fn warnings(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warn)
            .count()
    }
}

/// Validate one or all TMS plugins. Fails if any hard check fails.
pub fn validate_plugins(name: Option<&str>, config: Option<&Config>) -> Result<(), anyhow::Error> {
    let targets: Vec<String> = match name {
        Some(id) => vec![id.to_string()],
        None => collect_validation_targets(),
    };

    if targets.is_empty() {
        println!("No TMS plugins found on PATH (l10n4x-plugin-<id>).");
        println!("Install one or pass an explicit id: l10n4x plugin validate crowdin");
        return Ok(());
    }

    let mut reports = Vec::new();
    for id in &targets {
        reports.push(validate_plugin(id, config));
    }

    print_reports(&reports);

    if reports.iter().any(PluginReport::failed) {
        anyhow::bail!("plugin validation failed");
    }
    Ok(())
}

fn collect_validation_targets() -> Vec<String> {
    let mut ids = discover_plugin_ids_on_path();
    for (id, _) in KNOWN_PLUGINS {
        if !ids.iter().any(|x| x == id) {
            ids.push((*id).to_string());
        }
    }
    #[cfg(feature = "plugin-crowdin")]
    {
        if !ids.iter().any(|x| x == "crowdin") {
            ids.push("crowdin".to_string());
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

fn validate_plugin(id: &str, config: Option<&Config>) -> PluginReport {
    let mut checks = Vec::new();

    checks.push(check_id_format(id));
    checks.push(check_not_core_provider(id));

    let linked = in_process_plugin(id);
    let binary = which_plugin_binary_path(id);

    if linked {
        checks.push(Check {
            label: "availability",
            status: CheckStatus::Pass,
            detail: "linked in-process in this l10n4x build".to_string(),
        });
    } else if let Some(path) = &binary {
        checks.push(Check {
            label: "availability",
            status: CheckStatus::Pass,
            detail: format!("binary found at {}", path.display()),
        });
    } else {
        checks.push(Check {
            label: "availability",
            status: CheckStatus::Fail,
            detail: format!(
                "install l10n4x-plugin-{id} on PATH or build l10n4x with the plugin feature"
            ),
        });
    }

    if let Some(path) = binary.as_ref() {
        checks.push(check_executable(path));
        checks.extend(probe_cli(path));
    } else if linked {
        checks.push(Check {
            label: "cli-probe",
            status: CheckStatus::Skip,
            detail: "no standalone binary on PATH to probe --help".to_string(),
        });
    }

    if let Some(cfg) = config {
        checks.push(check_config_section(id, cfg));
    } else {
        checks.push(Check {
            label: "config",
            status: CheckStatus::Skip,
            detail: "no l10n4x.config.json in cwd".to_string(),
        });
    }

    PluginReport {
        id: id.to_string(),
        checks,
    }
}

fn check_id_format(id: &str) -> Check {
    if is_valid_plugin_id(id) {
        Check {
            label: "id-format",
            status: CheckStatus::Pass,
            detail: "matches [a-z][a-z0-9-]*".to_string(),
        }
    } else {
        Check {
            label: "id-format",
            status: CheckStatus::Fail,
            detail: "use lowercase letters, digits, hyphens; must start with a letter".to_string(),
        }
    }
}

fn check_not_core_provider(id: &str) -> Check {
    if CORE_PROVIDERS.contains(&id) {
        Check {
            label: "reserved-id",
            status: CheckStatus::Fail,
            detail: format!("'{id}' is a core provider, not a plugin id"),
        }
    } else {
        Check {
            label: "reserved-id",
            status: CheckStatus::Pass,
            detail: "not a reserved core provider".to_string(),
        }
    }
}

fn check_executable(path: &Path) -> Check {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) if meta.permissions().mode() & 0o111 != 0 => Check {
                label: "executable",
                status: CheckStatus::Pass,
                detail: "binary is executable".to_string(),
            },
            Ok(_) => Check {
                label: "executable",
                status: CheckStatus::Fail,
                detail: "binary is not executable (chmod +x)".to_string(),
            },
            Err(e) => Check {
                label: "executable",
                status: CheckStatus::Fail,
                detail: format!("cannot read binary metadata: {e}"),
            },
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Check {
            label: "executable",
            status: CheckStatus::Skip,
            detail: "executable bit check only on Unix".to_string(),
        }
    }
}

fn probe_cli(path: &Path) -> Vec<Check> {
    let mut checks = Vec::new();

    let help = Command::new(path)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match help {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
            checks.push(Check {
                label: "help",
                status: CheckStatus::Pass,
                detail: "--help exits 0".to_string(),
            });
            if text.contains("sync") {
                checks.push(Check {
                    label: "sync-subcommand",
                    status: CheckStatus::Pass,
                    detail: "CLI documents `sync`".to_string(),
                });
            } else {
                checks.push(Check {
                    label: "sync-subcommand",
                    status: CheckStatus::Fail,
                    detail: "CLI must expose `sync <export|import|push>`".to_string(),
                });
            }
        }
        Ok(out) => checks.push(Check {
            label: "help",
            status: CheckStatus::Fail,
            detail: format!("--help failed with status {}", out.status),
        }),
        Err(e) => checks.push(Check {
            label: "help",
            status: CheckStatus::Fail,
            detail: format!("failed to run --help: {e}"),
        }),
    }

    let info = Command::new(path)
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match info {
        Ok(status) if status.success() => checks.push(Check {
            label: "info-subcommand",
            status: CheckStatus::Pass,
            detail: "`info` subcommand exits 0".to_string(),
        }),
        Ok(status) => checks.push(Check {
            label: "info-subcommand",
            status: CheckStatus::Warn,
            detail: format!("`info` subcommand recommended (exited with {status})"),
        }),
        Err(e) => checks.push(Check {
            label: "info-subcommand",
            status: CheckStatus::Warn,
            detail: format!("could not run `info`: {e}"),
        }),
    }

    checks
}

fn check_config_section(id: &str, config: &Config) -> Check {
    if config.plugins.contains_key(id) {
        Check {
            label: "config",
            status: CheckStatus::Pass,
            detail: format!("plugins.{id} present in l10n4x.config.json"),
        }
    } else {
        Check {
            label: "config",
            status: CheckStatus::Warn,
            detail: format!("plugins.{id} missing (optional until API credentials needed)"),
        }
    }
}

fn print_reports(reports: &[PluginReport]) {
    for report in reports {
        println!("Validating plugin: {}", report.id);
        for check in &report.checks {
            let tag = match check.status {
                CheckStatus::Pass => "ok",
                CheckStatus::Warn => "warn",
                CheckStatus::Fail => "FAIL",
                CheckStatus::Skip => "skip",
            };
            println!("  [{tag:4}] {} — {}", check.label, check.detail);
        }
        let status = if report.failed() {
            "FAILED"
        } else if report.warnings() > 0 {
            "PASS (with warnings)"
        } else {
            "PASS"
        };
        println!("=> {}: {status}\n", report.id);
    }
}

pub fn is_valid_plugin_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn discover_plugin_ids_on_path() -> Vec<String> {
    let mut ids = Vec::new();
    let Some(paths) = std::env::var_os("PATH") else {
        return ids;
    };

    for dir in std::env::split_paths(&paths) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_prefix("l10n4x-plugin-") {
                if is_valid_plugin_id(id) && !ids.iter().any(|x| x == id) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    ids
}

fn which_plugin_binary_path(id: &str) -> Option<std::path::PathBuf> {
    let name = plugin_binary_name(id);
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_plugin_ids() {
        assert!(is_valid_plugin_id("crowdin"));
        assert!(is_valid_plugin_id("my-tms"));
        assert!(is_valid_plugin_id("lokalise2"));
    }

    #[test]
    fn rejects_invalid_plugin_ids() {
        assert!(!is_valid_plugin_id(""));
        assert!(!is_valid_plugin_id("Crowdin"));
        assert!(!is_valid_plugin_id("my_plugin"));
        assert!(!is_valid_plugin_id("9bad"));
        // `file` matches id syntax but is rejected by reserved-id check, not format.
        assert!(is_valid_plugin_id("file"));
    }
}
