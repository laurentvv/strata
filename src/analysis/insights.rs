//! Clean-up insights over a finished scan: well-known reclaimable folders, empty folders and a
//! size-by-age breakdown. Everything here is read-only; deleting stays an explicit user action.

use crate::scan::{NodeId, Tree};

/// How sure we are that deleting a folder loses nothing that cannot be recreated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Regenerated automatically: caches, build output, temporary files.
    Safe,
    /// Probably removable, but worth a look first: virtual environments, old Windows copies, the Recycle Bin.
    Check,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Safe => "Safe",
            Risk::Check => "Check first",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reclaimable {
    pub id: NodeId,
    /// Short reason shown next to the folder (for example "Node.js dependencies").
    pub kind: &'static str,
    pub size: u64,
    pub risk: Risk,
}

/// Something that must sit next to, or inside, a folder for a rule to apply (a bare `target` folder
/// is not necessarily a Rust build). Names are lower case; `*.ext` matches a suffix.
enum Marker {
    None,
    /// An entry of the folder's parent.
    Sibling(&'static [&'static str]),
    /// An entry of the folder itself.
    Inside(&'static [&'static str]),
}

struct Rule {
    /// Lower-case path components the folder's path must end with; `*` stands for any one name.
    path: &'static [&'static str],
    kind: &'static str,
    risk: Risk,
    marker: Marker,
}

const fn rule(path: &'static [&'static str], kind: &'static str, risk: Risk, marker: Marker) -> Rule {
    Rule { path, kind, risk, marker }
}

use Risk::{Check, Safe};

const RULES: &[Rule] = &[
    // Project folders, recognised by name (and a marker where the name alone is too common).
    // With a lock file the install is reproducible (`npm ci`); with only a package.json it is not guaranteed.
    rule(&["node_modules"], "Node.js dependencies", Safe, Marker::Sibling(&["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock", "npm-shrinkwrap.json"])),
    rule(&["node_modules"], "Node.js dependencies", Check, Marker::Sibling(&["package.json"])),
    rule(&["target"], "Rust build output", Safe, Marker::Sibling(&["cargo.toml"])),
    rule(&["__pycache__"], "Python bytecode cache", Safe, Marker::None),
    rule(&[".pytest_cache"], "pytest cache", Safe, Marker::None),
    rule(&[".mypy_cache"], "mypy cache", Safe, Marker::None),
    rule(&[".ruff_cache"], "Ruff cache", Safe, Marker::None),
    rule(&[".tox"], "tox environments", Safe, Marker::None),
    rule(&[".gradle"], "Gradle cache", Safe, Marker::None),
    rule(&["build"], "Gradle build output", Safe, Marker::Sibling(&["build.gradle", "build.gradle.kts"])),
    rule(&[".next"], "Next.js build cache", Safe, Marker::Sibling(&["package.json"])),
    rule(&[".nuxt"], "Nuxt build cache", Safe, Marker::Sibling(&["package.json"])),
    rule(&[".svelte-kit"], "SvelteKit build cache", Safe, Marker::Sibling(&["package.json"])),
    rule(&[".angular"], "Angular cache", Safe, Marker::Sibling(&["angular.json"])),
    rule(&[".parcel-cache"], "Parcel cache", Safe, Marker::None),
    rule(&["dist"], "JavaScript build output", Check, Marker::Sibling(&["package.json"])),
    rule(&["bin"], ".NET build output", Safe, Marker::Sibling(&["*.csproj", "*.fsproj", "*.vbproj"])),
    rule(&["obj"], ".NET build output", Safe, Marker::Sibling(&["*.csproj", "*.fsproj", "*.vbproj"])),
    rule(&[".vs"], "Visual Studio cache", Safe, Marker::Sibling(&["*.sln", "*.csproj"])),
    rule(&[".nuget"], "NuGet cache", Safe, Marker::None),
    rule(&["venv"], "Python virtual environment", Check, Marker::Inside(&["pyvenv.cfg"])),
    rule(&[".venv"], "Python virtual environment", Check, Marker::Inside(&["pyvenv.cfg"])),
    rule(&["env"], "Python virtual environment", Check, Marker::Inside(&["pyvenv.cfg"])),
    rule(&[".terraform"], "Terraform providers cache", Check, Marker::Sibling(&["*.tf"])),
    // Windows leftovers.
    rule(&["$recycle.bin"], "Recycle Bin", Check, Marker::None),
    rule(&["windows.old"], "Previous Windows installation", Check, Marker::None),
    rule(&["$windows.~bt"], "Windows upgrade leftovers", Check, Marker::None),
    rule(&["$windows.~ws"], "Windows upgrade leftovers", Check, Marker::None),
    // Well-known folders that only hold caches, recognised by the path they end with.
    rule(&["appdata", "local", "temp"], "Temporary files", Safe, Marker::None),
    rule(&["windows", "temp"], "Windows temporary files", Safe, Marker::None),
    rule(&["windows", "softwaredistribution", "download"], "Windows Update downloads", Safe, Marker::None),
    rule(&["programdata", "microsoft", "windows", "wer"], "Windows error reports", Safe, Marker::None),
    rule(&["appdata", "local", "crashdumps"], "Crash dumps", Safe, Marker::None),
    rule(&["windows", "minidump"], "Crash dumps", Safe, Marker::None),
    rule(&["appdata", "local", "microsoft", "windows", "inetcache"], "Internet cache", Safe, Marker::None),
    rule(&["appdata", "local", "pip", "cache"], "pip cache", Safe, Marker::None),
    rule(&["appdata", "local", "nuget", "v3-cache"], "NuGet cache", Safe, Marker::None),
    rule(&["appdata", "local", "yarn", "cache"], "Yarn cache", Safe, Marker::None),
    rule(&[".npm", "_cacache"], "npm cache", Safe, Marker::None),
    rule(&[".cargo", "registry"], "Cargo registry cache", Safe, Marker::None),
    rule(&[".m2", "repository"], "Maven repository cache", Check, Marker::None),
    rule(&["google", "chrome", "user data", "*", "cache"], "Chrome cache", Safe, Marker::None),
    rule(&["google", "chrome", "user data", "*", "code cache"], "Chrome cache", Safe, Marker::None),
    rule(&["google", "chrome", "user data", "*", "gpucache"], "Chrome cache", Safe, Marker::None),
    rule(&["microsoft", "edge", "user data", "*", "cache"], "Edge cache", Safe, Marker::None),
    rule(&["microsoft", "edge", "user data", "*", "code cache"], "Edge cache", Safe, Marker::None),
    rule(&["microsoft", "edge", "user data", "*", "gpucache"], "Edge cache", Safe, Marker::None),
];

fn component_matches(want: &str, name: &str) -> bool {
    want == "*" || name.eq_ignore_ascii_case(want)
}

/// Does the path of `id` end with the components `want` (the scanned root counts with all the
/// components of its own path)?
fn path_ends_with(tree: &Tree, id: NodeId, want: &[&str]) -> bool {
    let mut want = want.iter().rev();
    let mut cur = id;
    loop {
        let n = tree.get(cur);
        let name = tree.name(cur);
        if n.parent == crate::scan::NONE {
            for comp in name.rsplit(['\\', '/']).filter(|c| !c.is_empty()) {
                match want.next() {
                    None => return true,
                    Some(w) if component_matches(w, comp) => {}
                    Some(_) => return false,
                }
            }
            return want.next().is_none();
        }
        match want.next() {
            None => return true,
            Some(w) if component_matches(w, name) => {}
            Some(_) => return false,
        }
        cur = n.parent;
    }
}

/// `*.ext` matches a suffix, anything else an exact name (both case-insensitive, `pattern` lower case).
fn name_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some(suffix) => name.to_lowercase().ends_with(suffix),
        None => name.eq_ignore_ascii_case(pattern),
    }
}

/// Is the folder inside installed software (Program Files, AppData\Local\Programs, editor
/// extensions, language servers a program manages) or a global package location (`%APPDATA%\npm`,
/// `.bun`, `.nvm`...)? Dependency folders found there belong to a program or to tools the user
/// installed globally, and no project install brings them back: deleting them would break things.
fn inside_installed_software(tree: &Tree, id: NodeId) -> bool {
    let mut comps: Vec<String> = Vec::new();
    let mut cur = tree.get(id).parent;
    while cur != crate::scan::NONE {
        let name = tree.name(cur);
        if tree.get(cur).parent == crate::scan::NONE {
            comps.extend(name.split(['\\', '/']).filter(|c| !c.is_empty()).rev().map(str::to_lowercase));
        } else {
            comps.push(name.to_lowercase());
        }
        cur = tree.get(cur).parent;
    }
    // `comps` runs from the folder's parent up to the root.
    comps.iter().enumerate().any(|(i, c)| {
        matches!(c.as_str(), "program files" | "program files (x86)" | "windowsapps" | "extensions" | "languages" | ".bun" | ".volta" | ".nvm" | "nvm" | ".deno")
            || (c == "programs" && comps.get(i + 1).is_some_and(|p| p == "local"))
            || (c == "npm" && comps.get(i + 1).is_some_and(|p| p == "roaming"))
    })
}

fn rule_for(tree: &Tree, id: NodeId) -> Option<&'static Rule> {
    let n = tree.get(id);
    let name = tree.name(id);
    RULES.iter().find(|r| {
        if !r.path.last().is_some_and(|last| component_matches(last, name)) || !path_ends_with(tree, id, r.path) {
            return false;
        }
        // Project folders recognised by name alone are only offered inside the user's own projects.
        if r.path.len() == 1 && inside_installed_software(tree, id) {
            return false;
        }
        let any_entry = |parent: NodeId, patterns: &[&str]| tree.children(parent).iter().any(|&c| patterns.iter().any(|p| name_matches(p, tree.name(c))));
        match r.marker {
            Marker::None => true,
            Marker::Sibling(patterns) => n.parent != crate::scan::NONE && any_entry(n.parent, patterns),
            Marker::Inside(patterns) => any_entry(id, patterns),
        }
    })
}

/// Reclaimable folders, biggest first. A match is not searched further (its contents are already
/// counted in it).
pub fn reclaimable(tree: &Tree) -> Vec<Reclaimable> {
    let mut out = Vec::new();
    if tree.nodes.is_empty() {
        return out;
    }
    let mut stack = vec![Tree::ROOT];
    while let Some(id) = stack.pop() {
        let n = tree.get(id);
        if !n.is_dir || n.is_link {
            continue;
        }
        if id != Tree::ROOT {
            if let Some(rule) = rule_for(tree, id) {
                if n.size > 0 {
                    out.push(Reclaimable { id, kind: rule.kind, size: n.size, risk: rule.risk });
                }
                continue;
            }
        }
        stack.extend(tree.children(id).iter().copied());
    }
    out.sort_by(|a, b| b.size.cmp(&a.size).then(a.id.cmp(&b.id)));
    out
}

/// Parts of a machine where a big old file is part of the system or of installed software, not of the
/// user's data: never suggested for deletion.
fn in_system_area(tree: &Tree, id: NodeId) -> bool {
    let mut cur = tree.get(id).parent;
    while cur != crate::scan::NONE {
        let name = tree.name(cur);
        let comps: Vec<&str> = if tree.get(cur).parent == crate::scan::NONE { name.split(['\\', '/']).filter(|c| !c.is_empty()).collect() } else { vec![name] };
        if comps.iter().any(|c| {
            ["windows", "program files", "program files (x86)", "programdata", "windowsapps", "$recycle.bin", "system volume information"].iter().any(|s| c.eq_ignore_ascii_case(s))
        }) {
            return true;
        }
        cur = tree.get(cur).parent;
    }
    false
}

/// A big file nobody touched for a long time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldFile {
    pub id: NodeId,
    pub size: u64,
    /// Whole days since it was last modified.
    pub age_days: u64,
}

/// Files of at least `min_size` bytes last modified `min_age_days` ago or more, biggest first (at most
/// `limit`). Files inside folders that are already offered whole, in system areas or inside installed
/// software are left out.
pub fn large_old_files(tree: &Tree, now_secs: i64, min_size: u64, min_age_days: u64, limit: usize) -> Vec<OldFile> {
    let offered: std::collections::HashSet<NodeId> = reclaimable(tree).into_iter().map(|r| r.id).collect();
    let mut out: Vec<OldFile> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.is_dir && !n.is_link && n.size >= min_size && n.modified > 0)
        .map(|(i, n)| OldFile { id: i as NodeId, size: n.size, age_days: (now_secs - n.modified).max(0) as u64 / 86_400 })
        .filter(|f| f.age_days >= min_age_days)
        .filter(|f| {
            let mut cur = tree.get(f.id).parent;
            while cur != crate::scan::NONE {
                if offered.contains(&cur) {
                    return false;
                }
                cur = tree.get(cur).parent;
            }
            !in_system_area(tree, f.id) && !inside_installed_software(tree, f.id)
        })
        .collect();
    out.sort_by(|a, b| b.size.cmp(&a.size).then(a.id.cmp(&b.id)));
    out.truncate(limit);
    out
}

/// Folders that hold nothing at all (no files, no sub-folders), in tree order. Links and the
/// insides of reclaimable folders are skipped.
pub fn empty_dirs(tree: &Tree) -> Vec<NodeId> {
    let mut out = Vec::new();
    if tree.nodes.is_empty() {
        return out;
    }
    let mut stack = vec![Tree::ROOT];
    while let Some(id) = stack.pop() {
        let n = tree.get(id);
        if !n.is_dir || n.is_link {
            continue;
        }
        if id != Tree::ROOT && rule_for(tree, id).is_some() {
            continue; // already offered as a whole by `reclaimable`
        }
        if id != Tree::ROOT && tree.children(id).is_empty() {
            out.push(id);
        }
        stack.extend(tree.children(id).iter().copied());
    }
    out.sort_unstable();
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgeBucket {
    pub label: &'static str,
    pub files: u64,
    pub size: u64,
}

/// Bucket upper bounds in days, oldest bucket last (`u64::MAX`).
const AGE_BOUNDS: &[(&str, u64)] = &[
    ("Last 30 days", 30),
    ("1 to 6 months", 182),
    ("6 months to 1 year", 365),
    ("1 to 2 years", 730),
    ("2 to 5 years", 1826),
    ("More than 5 years", u64::MAX),
];

/// Size and file count per last-modified age. Files with an unknown date (0) are counted as the
/// oldest bucket.
pub fn age_buckets(tree: &Tree, now_secs: i64) -> Vec<AgeBucket> {
    let mut out: Vec<AgeBucket> = AGE_BOUNDS.iter().map(|(l, _)| AgeBucket { label: l, files: 0, size: 0 }).collect();
    for n in &tree.nodes {
        if n.is_dir {
            continue;
        }
        let days = if n.modified <= 0 { u64::MAX } else { (now_secs - n.modified).max(0) as u64 / 86_400 };
        let i = AGE_BOUNDS.iter().position(|(_, max)| days <= *max).unwrap_or(AGE_BOUNDS.len() - 1);
        out[i].files += 1;
        out[i].size += n.size;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_simple;
    use std::path::Path;

    fn write(p: &Path, n: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; n]).unwrap();
    }

    #[test]
    fn finds_known_folders_and_respects_sibling_rules() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("web/package.json"), 10);
        write(&r.join("web/package-lock.json"), 10);
        write(&r.join("web/node_modules/dep/index.js"), 5000);
        write(&r.join("web/node_modules/dep/node_modules/inner/x.js"), 900);
        write(&r.join("crate/Cargo.toml"), 10);
        write(&r.join("crate/target/debug/a.bin"), 3000);
        write(&r.join("plain/target/b.bin"), 7000); // no Cargo.toml: not a Rust build dir
        write(&r.join("py/__pycache__/m.pyc"), 100);
        std::fs::create_dir_all(r.join("web/node_modules_empty")).unwrap();
        let tree = scan_simple(r).tree;
        let found = reclaimable(&tree);
        let names: Vec<(&str, u64)> = found.iter().map(|f| (f.kind, f.size)).collect();
        assert_eq!(names, [("Node.js dependencies", 5900), ("Rust build output", 3000), ("Python bytecode cache", 100)]);
        assert!(found.iter().all(|f| f.risk == Safe));
        // Without a lock file the same folder still shows up, but as "check first".
        write(&r.join("loose/package.json"), 10);
        write(&r.join("loose/node_modules/x/i.js"), 4000);
        let tree = scan_simple(r).tree;
        let found = reclaimable(&tree);
        let loose = found.iter().find(|f| tree.path_of(f.id).to_string_lossy().contains("loose")).expect("loose project listed");
        assert_eq!((loose.kind, loose.risk, loose.size), ("Node.js dependencies", Check, 4000));
        assert!(found.iter().all(|f| tree.name(f.id) != "target" || tree.name(tree.get(f.id).parent) == "crate"));
    }

    #[test]
    fn every_rule_kind_and_risk_label_has_french_text() {
        let missing: Vec<&str> = RULES.iter().map(|r| r.kind).chain([Risk::Safe.label(), Risk::Check.label()]).filter(|t| !crate::i18n::is_translated("fr", t)).collect();
        assert!(missing.is_empty(), "no French for {missing:?}");
    }

    #[test]
    fn finds_well_known_cache_locations_by_path_and_marks_the_risk() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("Users/me/AppData/Local/Temp/a.tmp"), 4000);
        write(&r.join("Users/me/AppData/Local/pip/Cache/http/x"), 300);
        write(&r.join("Users/me/AppData/Local/Google/Chrome/User Data/Profile 2/Cache/f_000001"), 2000);
        write(&r.join("Users/me/AppData/Local/Google/Chrome/User Data/Profile 2/Bookmarks"), 50); // must stay
        write(&r.join("Users/me/.cargo/registry/cache/c.crate"), 1500);
        write(&r.join("Windows/SoftwareDistribution/Download/u.cab"), 9000);
        write(&r.join("Windows.old/Users/old.txt"), 7000);
        write(&r.join("dev/app/app.csproj"), 10);
        write(&r.join("dev/app/bin/Debug/app.dll"), 600);
        write(&r.join("dev/app/obj/o.bin"), 400);
        write(&r.join("dev/lib/bin/not_dotnet.bin"), 800); // no project file next to it
        write(&r.join("py/venv/pyvenv.cfg"), 10);
        write(&r.join("py/venv/lib/site.py"), 5000);
        write(&r.join("py/env/readme.txt"), 100); // "env" without pyvenv.cfg is a normal folder
        write(&r.join("Docs/Temp/keep.txt"), 123); // a Temp folder outside AppData\\Local
        let tree = scan_simple(r).tree;
        let found: Vec<(&str, Risk, u64)> = reclaimable(&tree).iter().map(|f| (f.kind, f.risk, f.size)).collect();
        let has = |kind: &str, risk: Risk, size: u64| found.contains(&(kind, risk, size));
        assert!(has("Temporary files", Safe, 4000), "{found:?}");
        assert!(has("pip cache", Safe, 300), "{found:?}");
        assert!(has("Chrome cache", Safe, 2000), "{found:?}");
        assert!(has("Cargo registry cache", Safe, 1500), "{found:?}");
        assert!(has("Windows Update downloads", Safe, 9000), "{found:?}");
        assert!(has("Previous Windows installation", Check, 7000), "{found:?}");
        assert!(has(".NET build output", Safe, 600) && has(".NET build output", Safe, 400), "{found:?}");
        assert!(has("Python virtual environment", Check, 5010), "{found:?}");
        // Nothing else: the profile data, the project without a .csproj, `env`, the unrelated Temp.
        assert_eq!(found.len(), 9, "{found:?}");
    }

    #[test]
    fn dependency_folders_of_installed_programs_are_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("Users/me/dev/site/package.json"), 10);
        write(&r.join("Users/me/dev/site/node_modules/a/i.js"), 1000); // the user's own project
        write(&r.join("Program Files/App/resources/app/package.json"), 10);
        write(&r.join("Program Files/App/resources/app/node_modules/b/i.js"), 2000);
        write(&r.join("Users/me/AppData/Local/Programs/Editor/resources/app/package.json"), 10);
        write(&r.join("Users/me/AppData/Local/Programs/Editor/resources/app/node_modules/c/i.js"), 3000);
        write(&r.join("Users/me/.vscode/extensions/ext-1.0/package.json"), 10);
        write(&r.join("Users/me/.vscode/extensions/ext-1.0/node_modules/d/i.js"), 4000);
        write(&r.join("Users/me/AppData/Local/Zed/languages/ls/package.json"), 10);
        write(&r.join("Users/me/AppData/Local/Zed/languages/ls/node_modules/e/i.js"), 5000);
        // Globally installed tools (`npm i -g`, bun, nvm): no project install brings them back.
        write(&r.join("Users/me/AppData/Roaming/npm/package.json"), 10);
        write(&r.join("Users/me/AppData/Roaming/npm/node_modules/gemini/i.js"), 6000);
        write(&r.join("Users/me/.bun/install/global/package.json"), 10);
        write(&r.join("Users/me/.bun/install/global/node_modules/f/i.js"), 7000);
        // No package.json next to it: not a project.
        write(&r.join("Users/me/tools/node_modules/g/i.js"), 8000);
        let tree = scan_simple(r).tree;
        let found = reclaimable(&tree);
        let paths: Vec<String> = found.iter().map(|f| tree.path_of(f.id).to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(found.len(), 1, "{paths:?}");
        assert!(paths[0].ends_with("Users/me/dev/site/node_modules"), "{paths:?}");
        // The same rule applies when the scan starts inside the user's profile.
        let tree = scan_simple(&r.join("Users/me/AppData/Local/Programs")).tree;
        assert!(reclaimable(&tree).is_empty(), "the root path itself says it is installed software");
    }

    #[test]
    fn the_scanned_root_counts_with_its_own_path() {
        let d = tempfile::tempdir().unwrap();
        let local = d.path().join("Users/me/AppData/Local");
        write(&local.join("Temp/a.tmp"), 100);
        write(&local.join("Other/b.bin"), 100);
        let tree = scan_simple(&local).tree;
        let found = reclaimable(&tree);
        assert_eq!(found.len(), 1, "scanning AppData\\Local directly still finds Temp");
        assert_eq!(found[0].kind, "Temporary files");
    }

    #[test]
    fn large_old_files_skip_young_small_system_and_already_offered_ones() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("Users/me/Videos/old_big.mkv"), 5000);
        write(&r.join("Users/me/Videos/young_big.mkv"), 6000);
        write(&r.join("Users/me/Videos/old_small.txt"), 10);
        write(&r.join("Windows/Installer/old_big.msi"), 9000);
        write(&r.join("Program Files/App/old_big.dll"), 9000);
        write(&r.join("proj/package.json"), 10);
        write(&r.join("proj/package-lock.json"), 10);
        write(&r.join("proj/node_modules/x/big_old.bin"), 7000); // inside a folder offered whole
        let mut tree = scan_simple(r).tree;
        let now = 2_000_000_000i64;
        let day = 86_400;
        for (path, age_days) in [
            (vec!["Users", "me", "Videos", "old_big.mkv"], 800),
            (vec!["Users", "me", "Videos", "young_big.mkv"], 20),
            (vec!["Users", "me", "Videos", "old_small.txt"], 900),
            (vec!["Windows", "Installer", "old_big.msi"], 900),
            (vec!["Program Files", "App", "old_big.dll"], 900),
            (vec!["proj", "node_modules", "x", "big_old.bin"], 900),
        ] {
            let id = tree.resolve_path(&path).unwrap();
            tree.nodes[id as usize].modified = now - age_days * day;
        }
        let found = large_old_files(&tree, now, 1000, 365, 10);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(tree.name(found[0].id), "old_big.mkv");
        assert_eq!((found[0].size, found[0].age_days), (5000, 800));
        assert_eq!(large_old_files(&tree, now, 1000, 365, 0).len(), 0, "the limit applies");
    }

    #[test]
    fn lists_only_truly_empty_folders() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        std::fs::create_dir_all(r.join("empty")).unwrap();
        std::fs::create_dir_all(r.join("parent/child")).unwrap();
        write(&r.join("full/f.bin"), 1);
        write(&r.join("app/package.json"), 1);
        std::fs::create_dir_all(r.join("app/node_modules/pkg/empty_inside")).unwrap();
        let tree = scan_simple(r).tree;
        let names: Vec<&str> = empty_dirs(&tree).iter().map(|&i| tree.name(i)).collect();
        assert!(names.contains(&"empty") && names.contains(&"child"));
        assert!(!names.contains(&"parent") && !names.contains(&"full"));
        assert!(!names.contains(&"empty_inside"), "inside a reclaimable folder");
    }

    #[test]
    fn age_buckets_split_by_modified_time() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("a.bin"), 100);
        write(&d.path().join("b.bin"), 50);
        let mut tree = scan_simple(d.path()).tree;
        let now = 1_000_000_000i64;
        let day = 86_400;
        let set = |tree: &mut Tree, name: &str, modified: i64| {
            let id = tree.resolve_path(&[name]).unwrap();
            tree.nodes[id as usize].modified = modified;
        };
        set(&mut tree, "a.bin", now - 3 * day); // last 30 days
        set(&mut tree, "b.bin", now - 3000 * day); // > 5 years
        let b = age_buckets(&tree, now);
        assert_eq!((b[0].files, b[0].size), (1, 100));
        assert_eq!((b[5].files, b[5].size), (1, 50));
        assert_eq!(b.iter().map(|x| x.files).sum::<u64>(), 2);
    }
}
