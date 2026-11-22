//! Headless CLI mode (`--cli <path> [--depth N]`): prints the scanned tree.

use crate::scan::{format_bytes, model::normalize_root, scan_simple, NodeId, Tree};
use colored::Colorize;
use std::path::PathBuf;

fn print_node(tree: &Tree, id: NodeId, prefix: &str, is_last: bool, max_depth: usize, depth: usize) {
    if depth > max_depth {
        return;
    }
    let n = tree.get(id);
    let parent_size = if n.parent == crate::scan::NONE { n.size } else { tree.get(n.parent).size };
    let percent = if parent_size > 0 { n.size as f64 / parent_size as f64 * 100.0 } else { 100.0 };
    let bar_width = 15usize;
    let filled = (((percent / 100.0) * bar_width as f64).round() as usize).min(bar_width);
    let bar = format!("[{}{}]", "█".repeat(filled).yellow(), "░".repeat(bar_width - filled).dimmed());
    let branch = if depth == 0 { "" } else if is_last { "└── " } else { "├── " };
    println!(
        "{}{}{} {:>10}  {} {:>6}  ({:>4} files, {:>3} dirs)",
        prefix.dimmed(),
        branch.dimmed(),
        tree.name(id).bold().green(),
        format_bytes(n.size).bold().white(),
        bar,
        format!("{:>5.1}%", percent).cyan(),
        n.files,
        n.dirs
    );
    let next_prefix = if depth == 0 {
        String::new()
    } else if is_last {
        format!("{}    ", prefix)
    } else {
        format!("{}│   ", prefix)
    };
    // Directories only, largest first.
    let mut kids: Vec<NodeId> = tree.children(id).iter().copied().filter(|&c| tree.get(c).is_dir).collect();
    kids.sort_by_key(|&c| std::cmp::Reverse(tree.get(c).size));
    let last = kids.len().saturating_sub(1);
    for (i, c) in kids.into_iter().enumerate() {
        print_node(tree, c, &next_prefix, i == last, max_depth, depth + 1);
    }
}

/// Parsed `--cli` arguments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CliOptions {
    pub target: PathBuf,
    pub max_depth: usize,
    /// `--export FILE` (csv / json / html chosen by extension).
    pub export: Option<(PathBuf, crate::export::Format)>,
    /// `--changes`: compare with the previous stored scan of the same root and store this one.
    pub changes: bool,
    /// `--cleanup`: list reclaimable folders (node_modules, build output, ...).
    pub cleanup: bool,
    /// `--json`: print a single machine-readable JSON object on stdout instead of formatted text.
    pub json: bool,
    /// `--fail-over SIZE`: exit with code 3 when the scanned total exceeds this byte threshold.
    pub fail_over: Option<u64>,
    /// `--fast`: read the NTFS Master File Table of a whole drive (administrator only), falling
    /// back to the regular scan with a message when it cannot be used.
    pub fast: bool,
}

pub fn parse_args(args: &[String]) -> Result<CliOptions, String> {
    let mut o = CliOptions { target: PathBuf::from("."), max_depth: 3, ..Default::default() };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--depth" => {
                i += 1;
                o.max_depth = args.get(i).and_then(|v| v.parse().ok()).ok_or("--depth needs a number")?;
            }
            "--export" => {
                i += 1;
                let p = PathBuf::from(args.get(i).ok_or("--export needs a file name")?);
                let fmt = crate::export::Format::from_path(&p)
                    .ok_or_else(|| format!("--export: unknown format for {} (use .csv, .json or .html)", p.display()))?;
                o.export = Some((p, fmt));
            }
            "--changes" => o.changes = true,
            "--cleanup" => o.cleanup = true,
            "--json" => o.json = true,
            "--fail-over" => {
                i += 1;
                let raw = args.get(i).ok_or("--fail-over needs a size (e.g. 10GB)")?;
                let bytes = crate::analysis::filter::parse_size(raw)
                    .map_err(|e| format!("--fail-over: {e}"))?
                    .ok_or("--fail-over needs a size (e.g. 10GB)")?;
                o.fail_over = Some(bytes);
            }
            "--fast" => o.fast = true,
            other if !other.starts_with('-') => o.target = PathBuf::from(other),
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }
    Ok(o)
}

/// Build the structured JSON summary for `--cli --json`.
fn build_json_report(
    res: &crate::scan::ScanResult,
    opts: &CliOptions,
    changes: Option<(i64, Vec<crate::analysis::snapshot::DirDelta>)>,
    now_secs: i64,
) -> serde_json::Value {
    let root = res.tree.root();
    let (size, alloc, files, dirs) = root.map_or((0, 0, 0, 0), |r| (r.size, r.alloc, r.files, r.dirs));
    let (hl_count, hl_alloc) = res.tree.hardlink_dup_stats(Tree::ROOT);
    let mut doc = serde_json::json!({
        "root": res.root.display().to_string(),
        "size_bytes": size,
        "allocated_bytes": alloc,
        "files": files,
        "folders": dirs,
        "errors": res.errors.len(),
        "cancelled": res.cancelled,
        "elapsed_ms": res.elapsed.as_millis() as u64,
    });
    if hl_alloc > 0 {
        doc["unique_allocated_bytes"] = serde_json::json!(alloc.saturating_sub(hl_alloc));
        doc["hardlink_duplicates"] = serde_json::json!(hl_count);
    }
    if opts.cleanup {
        use crate::analysis::insights;
        let found = insights::reclaimable(&res.tree);
        let total: u64 = found.iter().map(|f| f.size).sum();
        let reclaimable: Vec<_> = found
            .iter()
            .map(|f| {
                serde_json::json!({
                    "path": res.tree.path_of(f.id).display().to_string(),
                    "kind": f.kind,
                    "risk": f.risk.label(),
                    "size_bytes": f.size,
                })
            })
            .collect();
        let empty: Vec<String> = insights::empty_dirs(&res.tree).into_iter().map(|id| res.tree.path_of(id).display().to_string()).collect();
        let old: Vec<_> = insights::large_old_files(&res.tree, now_secs, 100 << 20, 365, 20)
            .into_iter()
            .map(|f| {
                serde_json::json!({
                    "path": res.tree.path_of(f.id).display().to_string(),
                    "size_bytes": f.size,
                    "age_days": f.age_days,
                })
            })
            .collect();
        doc["cleanup"] = serde_json::json!({
            "total_bytes": total,
            "reclaimable": reclaimable,
            "empty_dirs": empty,
            "large_old_files": old,
        });
    }
    if opts.changes {
        use crate::analysis::snapshot::ChangeKind;
        doc["changes"] = match changes {
            None => serde_json::json!({ "previous_taken": null, "items": [] }),
            Some((taken, list)) => {
                let items: Vec<_> = list
                    .iter()
                    .map(|c| {
                        let kind = match c.kind {
                            ChangeKind::Added => "added",
                            ChangeKind::Removed => "removed",
                            ChangeKind::Changed => "changed",
                        };
                        serde_json::json!({
                            "path": if c.path.is_empty() { "." } else { c.path.as_str() },
                            "kind": kind,
                            "old_size_bytes": c.old_size,
                            "new_size_bytes": c.new_size,
                            "delta_bytes": c.delta(),
                        })
                    })
                    .collect();
                serde_json::json!({ "previous_taken": taken, "items": items })
            }
        };
    }
    if let Some(limit) = opts.fail_over {
        doc["threshold"] = serde_json::json!({
            "limit_bytes": limit,
            "exceeded": size > limit,
        });
    }
    doc
}

fn print_changes(tree: &Tree) {
    use crate::analysis::snapshot;
    let Some(dir) = snapshot::history_dir() else {
        println!("  No settings folder available for the scan history.");
        return;
    };
    match snapshot::record_and_compare(&dir, tree, chrono::Utc::now().timestamp()) {
        None => println!("  First scan of this folder recorded; run again later to see changes."),
        Some((taken, changes)) => {
            println!("  Changes since the scan of {} ({} folders changed):", crate::scan::format_time(taken), changes.len());
            for c in changes.iter().take(25) {
                let d = c.delta();
                let sign = if d >= 0 { "+" } else { "-" };
                let path = if c.path.is_empty() { ".".to_string() } else { c.path.clone() };
                println!("  {sign}{:>10}  {}", format_bytes(d.unsigned_abs()), path);
            }
        }
    }
}

fn print_cleanup(tree: &Tree) {
    use crate::analysis::insights;
    let found = insights::reclaimable(tree);
    let total: u64 = found.iter().map(|f| f.size).sum();
    println!("  Reclaimable: {} in {} folders", format_bytes(total), found.len());
    for f in found.iter().take(50) {
        println!("  {:>10}  {:<9} {:<32} {}", format_bytes(f.size), f.risk.label(), f.kind, tree.path_of(f.id).display());
    }
    println!("  Empty folders: {}", insights::empty_dirs(tree).len());
    let old = insights::large_old_files(tree, chrono::Utc::now().timestamp(), 100 << 20, 365, 20);
    if !old.is_empty() {
        println!("  Large files not modified for over a year (check before deleting):");
        for f in old {
            println!("  {:>10}  {:>4} days  {}", format_bytes(f.size), f.age_days, tree.path_of(f.id).display());
        }
    }
}

pub fn run(args: &[String]) {
    let opts = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let target = opts.target.clone();
    let max_depth = opts.max_depth;
    let canon = std::fs::canonicalize(&target).unwrap_or(target);
    let canon = PathBuf::from(crate::scan::model::strip_verbatim(&canon.to_string_lossy()));
    let abs = normalize_root(&canon);
    if !opts.json {
        println!("\n  {} - {}", "Strata".bold().yellow(), "CLI mode".white());
        println!("  Target: {}\n", abs.display().to_string().cyan());
    }
    let res = if opts.fast {
        let (progress, cancel) = (crate::scan::walker::Progress::default(), std::sync::atomic::AtomicBool::new(false));
        match crate::scan::mft::try_fast_scan(&abs, &progress, &cancel, &[]) {
            Ok(r) => {
                if !opts.json {
                    println!("  Mode: NTFS Master File Table\n");
                }
                r
            }
            Err(msg) => {
                eprintln!("  {msg}\n");
                scan_simple(&abs)
            }
        }
    } else {
        scan_simple(&abs)
    };
    let root = res.tree.root().expect("root");
    let now_secs = chrono::Utc::now().timestamp();
    if opts.json {
        let changes = if opts.changes {
            crate::analysis::snapshot::history_dir().and_then(|dir| crate::analysis::snapshot::record_and_compare(&dir, &res.tree, now_secs))
        } else {
            None
        };
        let doc = build_json_report(&res, &opts, changes, now_secs);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into()));
    } else {
        print_node(&res.tree, 0, "", true, max_depth, 0);
        let (hl_count, hl_alloc) = res.tree.hardlink_dup_stats(Tree::ROOT);
        let unique_note = if hl_alloc > 0 {
            format!(", {} unique across {} hard links", format_bytes(root.alloc.saturating_sub(hl_alloc)), hl_count)
        } else {
            String::new()
        };
        println!(
            "\n  Scanned: {} ({} on disk{unique_note}) in {} files across {} directories; {} errors",
            format_bytes(root.size).bold().white(),
            format_bytes(root.alloc),
            root.files.to_string().cyan(),
            root.dirs.to_string().cyan(),
            res.errors.len()
        );
        println!("  Time elapsed: {}\n", format!("{:.2?}", res.elapsed).bold().green());
        let denied = crate::scan::access_denied_count(&res.errors);
        if denied > 0 && cfg!(windows) && !crate::scan::mft::is_elevated() {
            eprintln!("  Note: {denied} folders could not be read (access denied). Run Strata from an elevated terminal for a complete result.\n");
        }
        if opts.cleanup {
            print_cleanup(&res.tree);
            println!();
        }
        if opts.changes {
            print_changes(&res.tree);
            println!();
        }
    }
    if let Some((path, fmt)) = &opts.export {
        match crate::export::export_to_file(&res.tree, Tree::ROOT, *fmt, 8, path) {
            Ok(()) => {
                if !opts.json {
                    println!("  Exported to {}\n", path.display());
                }
            }
            Err(e) => {
                eprintln!("error: cannot write {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    }
    if let Some(limit) = opts.fail_over {
        if root.size > limit {
            eprintln!("threshold exceeded: {} > {}", format_bytes(root.size), format_bytes(limit));
            std::process::exit(3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_rejects_bad_ones() {
        let o = parse_args(&a(&["C:\\data", "-d", "5", "--export", "out.CSV", "--changes", "--cleanup"])).unwrap();
        assert_eq!((o.max_depth, o.changes, o.cleanup, o.fast), (5, true, true, false));
        assert!(parse_args(&a(&["D:\\", "--fast"])).unwrap().fast);
        assert_eq!(o.target, PathBuf::from("C:\\data"));
        assert_eq!(o.export, Some((PathBuf::from("out.CSV"), crate::export::Format::Csv)));
        assert_eq!(parse_args(&a(&[])).unwrap().max_depth, 3);
        assert!(parse_args(&a(&["--export", "x.txt"])).unwrap_err().contains("unknown format"));
        assert!(parse_args(&a(&["--export"])).is_err());
        assert!(parse_args(&a(&["-d", "many"])).is_err());
        assert!(parse_args(&a(&["--nope"])).unwrap_err().contains("--nope"));
        let j = parse_args(&a(&["C:\\data", "--json", "--fail-over", "10MB"])).unwrap();
        assert!(j.json && j.fail_over == Some(10 * 1024 * 1024));
        assert!(parse_args(&a(&["--fail-over"])).is_err());
        assert!(parse_args(&a(&["--fail-over", "bogus"])).is_err());
    }

    #[test]
    fn json_report_includes_cleanup_changes_and_threshold() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("web/node_modules/pkg")).unwrap();
        std::fs::write(d.path().join("web/package-lock.json"), b"{}").unwrap();
        std::fs::write(d.path().join("web/node_modules/pkg/a.js"), vec![1u8; 4096]).unwrap();
        let res = scan_simple(d.path());
        let opts = CliOptions { cleanup: true, changes: true, json: true, fail_over: Some(1024), ..Default::default() };
        let doc = build_json_report(&res, &opts, None, 2_000_000_000);
        assert_eq!(doc["size_bytes"], 4098);
        assert_eq!(doc["cleanup"]["total_bytes"], 4096);
        assert_eq!(doc["cleanup"]["reclaimable"][0]["kind"], "Node.js dependencies");
        assert!(doc["changes"]["previous_taken"].is_null());
        assert_eq!(doc["threshold"]["limit_bytes"], 1024);
        assert_eq!(doc["threshold"]["exceeded"], true);
    }

    #[test]
    fn export_format_follows_the_extension() {
        use crate::export::Format;
        assert_eq!(Format::from_path(std::path::Path::new("r.htm")), Some(Format::Html));
        assert_eq!(Format::from_path(std::path::Path::new("r.JSON")), Some(Format::Json));
        assert_eq!(Format::from_path(std::path::Path::new("noext")), None);
    }
}
