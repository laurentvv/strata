//! In-memory tree model: an arena of nodes addressed by `u32` indices.

use crate::settings::Units;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

#[derive(Debug, Clone)]
pub struct Node {
    pub parent: NodeId,
    /// Logical size in bytes (aggregated for directories).
    pub size: u64,
    /// Size allocated on disk in bytes (aggregated for directories).
    pub alloc: u64,
    /// Number of files below (directories only; 0 for a file node).
    pub files: u64,
    /// Number of sub-directories below.
    pub dirs: u64,
    /// Last modified, seconds since Unix epoch (0 when unknown).
    pub modified: i64,
    /// Raw file attributes (Windows attribute bits; 0 elsewhere).
    pub attrs: u32,
    pub is_dir: bool,
    /// Symlink / junction / reparse point: listed but never followed.
    pub is_link: bool,
    /// Live-scan snapshots only: the directory is still being read (its totals are still growing).
    pub scanning: bool,
    /// Range of this node's name inside `Tree::names` (use `Tree::name`).
    pub(crate) name_off: u32,
    pub(crate) name_len: u32,
    /// Range of this node's child list inside `Tree::kids` (use `Tree::children`).
    pub(crate) child_start: u32,
    pub(crate) child_len: u32,
}

impl Node {
    /// A zeroed node below `parent`; callers override the fields they know and add it with
    /// `Tree::push_node`.
    pub fn blank(parent: NodeId, is_dir: bool) -> Node {
        Node {
            parent,
            size: 0,
            alloc: 0,
            files: 0,
            dirs: 0,
            modified: 0,
            attrs: 0,
            is_dir,
            is_link: false,
            scanning: false,
            name_off: 0,
            name_len: 0,
            child_start: 0,
            child_len: 0,
        }
    }
}

/// Direct (non-recursive) files of one directory, shown as the "[N Files]" pseudo-row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirectFiles {
    pub count: u64,
    pub size: u64,
    pub alloc: u64,
    pub modified: i64,
}

/// A ranked file of a live snapshot (file nodes are not part of snapshots).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedFile {
    /// Directory (node id of the same snapshot) holding the file.
    pub dir: NodeId,
    pub name: String,
    pub size: u64,
    pub modified: i64,
}

/// Per-extension aggregate of a live snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtAgg {
    pub ext: String,
    pub count: u64,
    pub size: u64,
    pub alloc: u64,
}

/// File-level data streamed with a partial snapshot: bounded in size whatever the file count
/// (aggregates + top-N lists), so a snapshot never clones millions of nodes.
#[derive(Debug, Clone, Default)]
pub struct LiveStats {
    /// Indexed by node id of the snapshot.
    pub direct: Vec<DirectFiles>,
    pub types: Vec<ExtAgg>,
    pub largest: Vec<RankedFile>,
    pub oldest: Vec<RankedFile>,
}

#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// All node names, concatenated (addressed by `Node::name_off` / `name_len`).
    pub(crate) names: String,
    /// Flat child lists (addressed by `Node::child_start` / `child_len`).
    pub(crate) kids: Vec<NodeId>,
    /// Present on live-scan snapshots only (the final tree retains its file nodes instead).
    pub live: Option<Box<LiveStats>>,
}

impl Tree {
    pub const ROOT: NodeId = 0;

    /// One-node tree shown from the very first frame of a scan (counters are patched in live).
    pub fn placeholder(root_name: &str) -> Tree {
        let mut t = Tree { live: Some(Box::default()), ..Tree::default() };
        t.push_node(root_name, Node { scanning: true, ..Node::blank(NONE, true) });
        t
    }

    /// Empty tree with room for exactly `nodes` nodes, `name_bytes` bytes of names and `kids` child links.
    pub fn with_capacity(nodes: usize, name_bytes: usize, kids: usize) -> Tree {
        Tree { nodes: Vec::with_capacity(nodes), names: String::with_capacity(name_bytes), kids: Vec::with_capacity(kids), live: None }
    }

    /// Append a node (its name is copied into the shared name buffer) and return its id. The child
    /// list starts empty; fill it with `set_children`.
    pub fn push_node(&mut self, name: &str, mut node: Node) -> NodeId {
        let id = self.nodes.len() as NodeId;
        node.name_off = self.names.len() as u32;
        node.name_len = name.len() as u32;
        node.child_start = self.kids.len() as u32;
        node.child_len = 0;
        self.names.push_str(name);
        self.nodes.push(node);
        id
    }

    /// Replace the child list of `id` by `children` (appended at the end of the flat array; the
    /// previous list becomes unused space).
    pub fn set_children(&mut self, id: NodeId, children: &[NodeId]) {
        let start = self.kids.len() as u32;
        self.kids.extend_from_slice(children);
        let n = &mut self.nodes[id as usize];
        n.child_start = start;
        n.child_len = children.len() as u32;
    }

    /// Rename a node (the new name is appended to the name buffer; the old one becomes unused space).
    pub fn set_name(&mut self, id: NodeId, name: &str) {
        let off = self.names.len() as u32;
        self.names.push_str(name);
        let n = &mut self.nodes[id as usize];
        n.name_off = off;
        n.name_len = name.len() as u32;
    }

    pub fn name(&self, id: NodeId) -> &str {
        let n = &self.nodes[id as usize];
        &self.names[n.name_off as usize..(n.name_off + n.name_len) as usize]
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        let n = &self.nodes[id as usize];
        &self.kids[n.child_start as usize..(n.child_start + n.child_len) as usize]
    }

    /// Files held directly by a directory: from the stream during a scan, from the retained file
    /// nodes afterwards.
    pub fn direct_files(&self, id: NodeId) -> DirectFiles {
        if let Some(l) = &self.live {
            return l.direct.get(id as usize).copied().unwrap_or_default();
        }
        let mut d = DirectFiles::default();
        for &c in self.children(id) {
            let n = self.get(c);
            if !n.is_dir {
                d.count += 1;
                d.size += n.size;
                d.alloc += n.alloc;
                d.modified = d.modified.max(n.modified);
            }
        }
        d
    }

    /// Individual file nodes can be listed (false for live snapshots).
    pub fn has_file_nodes(&self) -> bool {
        self.live.is_none()
    }

    pub fn root(&self) -> Option<&Node> {
        self.nodes.first()
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    /// Full path of a node; the root node's name is the scanned root path.
    pub fn path_of(&self, id: NodeId) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != NONE {
            parts.push(self.name(cur));
            cur = self.get(cur).parent;
        }
        let mut p = PathBuf::new();
        for part in parts.iter().rev() {
            p.push(part);
        }
        p
    }

    /// Names from the root's first child down to `id` (the root itself is the empty path).
    pub fn name_path(&self, id: NodeId) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != NONE && cur != Self::ROOT {
            parts.push(self.name(cur));
            cur = self.get(cur).parent;
        }
        parts.reverse();
        parts
    }

    /// Node reached by following `path` (names below the root); the inverse of `name_path`.
    pub fn resolve_path(&self, path: &[&str]) -> Option<NodeId> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut cur = Self::ROOT;
        for name in path {
            cur = self.find_child(cur, name)?;
        }
        Some(cur)
    }

    /// Detach a subtree from its parent and subtract its totals from all ancestors.
    pub fn remove_subtree(&mut self, id: NodeId) {
        if id == Self::ROOT || id as usize >= self.nodes.len() {
            return;
        }
        let (parent, size, alloc, files, dirs, is_dir) = {
            let n = self.get(id);
            (n.parent, n.size, n.alloc, n.files, n.dirs, n.is_dir)
        };
        if parent == NONE {
            return;
        }
        let rest: Vec<NodeId> = self.children(parent).iter().copied().filter(|&c| c != id).collect();
        self.set_children(parent, &rest);
        self.nodes[id as usize].parent = NONE;
        let (df, dd) = if is_dir { (files, dirs + 1) } else { (1, 0) };
        let mut cur = parent;
        while cur != NONE {
            let a = &mut self.nodes[cur as usize];
            a.size = a.size.saturating_sub(size);
            a.alloc = a.alloc.saturating_sub(alloc);
            a.files = a.files.saturating_sub(df);
            a.dirs = a.dirs.saturating_sub(dd);
            cur = a.parent;
        }
    }

    /// Replace directory `id` by a freshly scanned `sub` tree (rooted at the same folder) and add
    /// its totals to all ancestors. Returns the new node id, `None` when `id` cannot be replaced.
    pub fn graft(&mut self, id: NodeId, mut sub: Tree) -> Option<NodeId> {
        if id == Self::ROOT || id as usize >= self.nodes.len() || sub.nodes.is_empty() || sub.live.is_some() {
            return None;
        }
        let parent = self.get(id).parent;
        if parent == NONE {
            return None;
        }
        // The grafted root keeps the name of the node it replaces (same buffer range, no copy).
        let (name_off, name_len) = (self.get(id).name_off, self.get(id).name_len);
        self.remove_subtree(id);
        let off = self.nodes.len() as NodeId;
        let (name_shift, kid_shift) = (self.names.len() as u32, self.kids.len() as u32);
        for n in &mut sub.nodes {
            if n.parent != NONE {
                n.parent += off;
            }
            n.name_off += name_shift;
            n.child_start += kid_shift;
        }
        self.names.push_str(&sub.names);
        self.kids.extend(sub.kids.iter().map(|&c| c + off));
        sub.nodes[0].name_off = name_off;
        sub.nodes[0].name_len = name_len;
        sub.nodes[0].parent = parent;
        let (size, alloc, files, dirs) = (sub.nodes[0].size, sub.nodes[0].alloc, sub.nodes[0].files, sub.nodes[0].dirs + 1);
        self.nodes.extend(sub.nodes);
        let mut siblings = self.children(parent).to_vec();
        siblings.push(off);
        self.set_children(parent, &siblings);
        let mut cur = parent;
        while cur != NONE {
            let a = &mut self.nodes[cur as usize];
            a.size += size;
            a.alloc += alloc;
            a.files += files;
            a.dirs += dirs;
            cur = a.parent;
        }
        Some(off)
    }

    #[allow(dead_code)]
    pub fn find_child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        self.children(parent).iter().copied().find(|&c| self.name(c) == name)
    }
}

/// Translate node ids of `old` into the equivalent ids of `new` by path (snapshots rebuild indices);
/// nodes that no longer exist are dropped.
pub fn remap_ids(old: &Tree, new: &Tree, ids: &HashSet<NodeId>) -> HashSet<NodeId> {
    ids.iter()
        .filter(|&&id| (id as usize) < old.nodes.len())
        .filter_map(|&id| new.resolve_path(&old.name_path(id)))
        .collect()
}

/// Turn the verbatim form returned by `fs::canonicalize` back into a normal Win32 path:
/// `\\?\C:\x` becomes `C:\x` and `\\?\UNC\server\share\x` becomes `\\server\share\x`.
pub fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    }
}

/// Clean up a path typed or pasted into the path box: surrounding spaces and quotes (Explorer's
/// "Copy as path" adds them) are removed and `%VARIABLE%` references are expanded.
pub fn clean_path_input(s: &str) -> String {
    let t = s.trim().trim_matches('"').trim();
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => match std::env::var(&after[..j]) {
                Ok(v) => {
                    out.push_str(&v);
                    rest = &after[j + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Normalise a bare drive such as "C:" to "C:\".
pub fn normalize_root(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.len() == 2 && s.ends_with(':') {
        PathBuf::from(format!("{}\\", s))
    } else {
        p.to_path_buf()
    }
}

static CURRENT_UNITS: AtomicU8 = AtomicU8::new(0);

/// Global display unit used by `format_bytes` (set by the GUI from the settings).
pub fn set_units(u: Units) {
    CURRENT_UNITS.store(u.to_u8(), Ordering::Relaxed);
}

pub fn current_units() -> Units {
    Units::from_u8(CURRENT_UNITS.load(Ordering::Relaxed))
}

/// Format a size with the globally selected unit and language.
pub fn format_bytes(bytes: u64) -> String {
    crate::fmt::format_bytes_in(bytes, current_units(), crate::fmt::loc())
}

/// Format Unix seconds as date and time in the machine's local time zone (language-aware).
pub fn format_time(secs: i64) -> String {
    crate::fmt::format_datetime_in(secs, &chrono::Local, crate::fmt::loc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_input_is_cleaned() {
        assert_eq!(clean_path_input("  \"C:\\Program Files\\App\"  "), "C:\\Program Files\\App");
        assert_eq!(clean_path_input("D:\\data"), "D:\\data");
        std::env::set_var("UAR_TEST_DIR", "E:\\xx");
        assert_eq!(clean_path_input("%UAR_TEST_DIR%\\sub"), "E:\\xx\\sub");
        assert_eq!(clean_path_input("%NO_SUCH_VAR_42%\\a"), "%NO_SUCH_VAR_42%\\a");
        assert_eq!(clean_path_input("100%"), "100%");
        assert_eq!(clean_path_input("%%"), "%%");
    }

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(strip_verbatim(r"\\?\C:\Data\x"), r"C:\Data\x");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\dir"), r"\\srv\share\dir");
        assert_eq!(strip_verbatim(r"D:\plain"), r"D:\plain");
        assert_eq!(strip_verbatim(r"\\srv\share"), r"\\srv\share");
    }

    #[test]
    fn graft_matches_a_full_rescan() {
        use crate::scan::scan_simple;
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("a/deep")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("a/deep/x.bin"), vec![1u8; 1000]).unwrap();
        std::fs::write(root.join("b/y.bin"), vec![1u8; 500]).unwrap();
        let mut tree = scan_simple(root).tree;
        // Change folder `a` on disk, then rescan only it and graft.
        std::fs::write(root.join("a/new.bin"), vec![1u8; 4000]).unwrap();
        std::fs::remove_file(root.join("a/deep/x.bin")).unwrap();
        let a = tree.resolve_path(&["a"]).unwrap();
        let sub = scan_simple(&root.join("a")).tree;
        let new_a = tree.graft(a, sub).unwrap();
        assert_eq!(tree.name(new_a), "a");
        assert_eq!(tree.get(new_a).parent, Tree::ROOT);
        let full = scan_simple(root).tree;
        let (g, f) = (tree.get(Tree::ROOT), full.get(Tree::ROOT));
        assert_eq!((g.size, g.alloc, g.files, g.dirs), (f.size, f.alloc, f.files, f.dirs));
        assert_eq!(tree.resolve_path(&["a", "new.bin"]).map(|i| tree.get(i).size), Some(4000));
        assert!(tree.graft(Tree::ROOT, Tree::default()).is_none());
    }

    #[test]
    fn direct_files_from_nodes_and_placeholder() {
        let mut t = Tree::placeholder("R");
        assert_eq!(t.direct_files(0), DirectFiles::default());
        assert!(!t.has_file_nodes() && t.nodes[0].scanning);
        let mut ls = LiveStats::default();
        ls.direct.push(DirectFiles { count: 3, size: 30, alloc: 40, modified: 0 });
        t.live = Some(Box::new(ls));
        assert_eq!(t.direct_files(0).count, 3);
        // final tree: derived from the retained file children (links to files count, dirs do not)
        let file = |name: &str, parent, size, modified| (name.to_string(), Node { is_dir: false, size, alloc: size, modified, ..Node::blank(parent, false) }, vec![]);
        let f = build(vec![dir("R", NONE, vec![1, 2, 3]), file("a", 0, 5, 7), file("b", 0, 6, 9), dir("d", 0, vec![])]);
        assert!(f.has_file_nodes());
        assert_eq!(f.direct_files(0), DirectFiles { count: 2, size: 11, alloc: 11, modified: 9 });
    }

    type Spec = (String, Node, Vec<NodeId>);

    fn dir(name: &str, parent: NodeId, children: Vec<NodeId>) -> Spec {
        (name.to_string(), Node::blank(parent, true), children)
    }

    /// Build a tree by hand from (name, node, children) triples, in id order.
    fn build(specs: Vec<Spec>) -> Tree {
        let mut t = Tree::default();
        for (name, node, _) in &specs {
            t.push_node(name, node.clone());
        }
        for (i, (_, _, kids)) in specs.iter().enumerate() {
            t.set_children(i as NodeId, kids);
        }
        t
    }

    #[test]
    fn expanded_state_is_mapped_by_path_not_index() {
        // old: root -> [a -> [b], c]
        let old = build(vec![dir("R", NONE, vec![1, 3]), dir("a", 0, vec![2]), dir("b", 1, vec![]), dir("c", 0, vec![])]);
        // new snapshot rebuilt in another order, with an extra directory: root -> [z, c, a -> [b]]
        let new = build(vec![dir("R", NONE, vec![1, 2, 3]), dir("z", 0, vec![]), dir("c", 0, vec![]), dir("a", 0, vec![4]), dir("b", 3, vec![])]);
        let expanded: HashSet<NodeId> = [0, 1, 2].into_iter().collect(); // R, a, b
        let mapped = remap_ids(&old, &new, &expanded);
        let want: HashSet<NodeId> = [0, 3, 4].into_iter().collect();
        assert_eq!(mapped, want);
        // a vanished directory is dropped instead of aliasing another node
        let gone = build(vec![dir("R", NONE, vec![1]), dir("c", 0, vec![])]);
        let mapped = remap_ids(&old, &gone, &expanded);
        assert_eq!(mapped, [0].into_iter().collect());
        assert_eq!(old.name_path(2), vec!["a", "b"]);
    }

    #[test]
    fn node_is_compact() {
        assert!(std::mem::size_of::<Node>() <= 72, "Node is {} bytes", std::mem::size_of::<Node>());
    }

    #[test]
    fn name_and_children_accessors() {
        let t = build(vec![dir("R", NONE, vec![1, 2]), dir("alpha", 0, vec![]), dir("béta-日本", 0, vec![3]), dir("", 2, vec![])]);
        assert_eq!(t.name(0), "R");
        assert_eq!(t.name(1), "alpha");
        assert_eq!(t.name(2), "béta-日本");
        assert_eq!(t.name(3), "");
        assert_eq!(t.children(0), &[1, 2]);
        assert_eq!(t.children(1), &[] as &[NodeId]);
        assert_eq!(t.children(2), &[3]);
        assert_eq!(t.find_child(0, "béta-日本"), Some(2));
        assert_eq!(t.find_child(2, ""), Some(3));
        assert_eq!(t.find_child(0, "nope"), None);
        assert_eq!(t.name_path(3), vec!["béta-日本", ""]);
        assert_eq!(t.resolve_path(&["béta-日本", ""]), Some(3));
        assert_eq!(t.path_of(2), PathBuf::from("R").join("béta-日本"));
    }

    #[test]
    fn set_children_replaces_the_list() {
        let mut t = build(vec![dir("R", NONE, vec![1, 2]), dir("a", 0, vec![]), dir("b", 0, vec![])]);
        t.set_children(0, &[2]);
        assert_eq!(t.children(0), &[2]);
        t.set_children(0, &[]);
        assert!(t.children(0).is_empty());
        t.set_children(1, &[2, 1]);
        assert_eq!(t.children(1), &[2, 1]);
        assert_eq!(t.children(2), &[] as &[NodeId]);
    }

    #[test]
    fn remove_then_graft_round_trips_with_unicode_names() {
        use crate::scan::scan_simple;
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("dossier é/日本")).unwrap();
        std::fs::create_dir_all(root.join("z")).unwrap();
        std::fs::write(root.join("dossier é/日本/ñ.bin"), vec![1u8; 700]).unwrap();
        std::fs::write(root.join("dossier é/top.txt"), vec![1u8; 30]).unwrap();
        std::fs::write(root.join("z/w.bin"), vec![1u8; 9]).unwrap();
        let mut tree = scan_simple(root).tree;
        let dir_id = tree.resolve_path(&["dossier é"]).unwrap();
        tree.remove_subtree(dir_id);
        assert!(tree.resolve_path(&["dossier é"]).is_none());
        assert_eq!(tree.get(Tree::ROOT).files, 1);
        let sub = scan_simple(&root.join("dossier é")).tree;
        let new_id = tree.graft(dir_id, sub);
        // the removed node is detached (parent NONE), so grafting it again is refused
        assert!(new_id.is_none());
        // graft over a live node instead: rescan `z` and graft the unicode folder's content in place
        let z = tree.resolve_path(&["z"]).unwrap();
        std::fs::write(root.join("z/extra.bin"), vec![1u8; 100]).unwrap();
        let sub = scan_simple(&root.join("z")).tree;
        let nz = tree.graft(z, sub).unwrap();
        assert_eq!(tree.name(nz), "z");
        assert_eq!(tree.resolve_path(&["z", "extra.bin"]).map(|i| tree.get(i).size), Some(100));
        // full round trip against a fresh rescan
        let mut tree = scan_simple(root).tree;
        let a = tree.resolve_path(&["dossier é"]).unwrap();
        let sub = scan_simple(&root.join("dossier é")).tree;
        let na = tree.graft(a, sub).unwrap();
        assert_eq!(tree.name(na), "dossier é");
        let full = scan_simple(root).tree;
        let (g, f) = (tree.get(Tree::ROOT), full.get(Tree::ROOT));
        assert_eq!((g.size, g.alloc, g.files, g.dirs), (f.size, f.alloc, f.files, f.dirs));
        assert_eq!(tree.resolve_path(&["dossier é", "日本", "ñ.bin"]).map(|i| tree.get(i).size), Some(700));
        let mut names_g: Vec<&str> = tree.children(Tree::ROOT).iter().map(|&c| tree.name(c)).collect();
        let mut names_f: Vec<&str> = full.children(Tree::ROOT).iter().map(|&c| full.name(c)).collect();
        names_g.sort();
        names_f.sort();
        assert_eq!(names_g, names_f);
    }
}
