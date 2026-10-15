//! Parallel directory walker. Every entry goes straight into shared compact stores (fixed-size
//! node records plus one name buffer each); the final arena `Tree` is assembled from them.

use super::model::{DirectFiles, ExtAgg, LiveStats, Node, NodeId, RankedFile, Tree, NONE};
use rayon::prelude::*;
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
#[cfg(windows)]
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
#[cfg(windows)]
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
#[cfg(windows)]
const CLUSTER: u64 = 4096;

/// Live counters shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub current: Mutex<String>,
}

#[derive(Debug, Clone)]
pub struct ScanError {
    pub path: PathBuf,
    pub message: String,
}

pub struct ScanResult {
    pub root: PathBuf,
    pub tree: Tree,
    pub errors: Vec<ScanError>,
    pub cancelled: bool,
    pub elapsed: std::time::Duration,
}

/// Directory skeleton of the live arena: one fixed-size record per directory plus ONE shared name
/// buffer. A record is created the moment its directory is discovered (`scanning` set); its direct
/// totals (`size`/`alloc`/`files`/`dirs` = direct files, file links, directory links) are filled in
/// once its entries have been read. The sub-directories of a directory are registered in a single
/// call, so their ids are consecutive: `child_start`/`child_len` of the record hold that id range.
#[derive(Default)]
struct LiveDirs {
    nodes: Vec<Node>,
    names: String,
    /// Per directory: (first index, count) of its non-directory entries in the file store.
    files: Vec<[u32; 2]>,
}

/// File and link entries of every directory, appended one directory at a time. Records are kept in
/// fixed-size chunks (no big reallocation while scanning, and the final build can free them as it
/// goes); names live in one shared buffer.
#[derive(Default)]
struct FileStore {
    chunks: Vec<Vec<Node>>,
    names: String,
    total: u32,
}

const CHUNK: usize = 8192;

/// Shared, lock-protected directory skeleton that the ticker turns into partial `Tree` snapshots.
/// Children always have a larger index than their parent.
struct Live {
    dirs: Mutex<LiveDirs>,
    store: Mutex<FileStore>,
    /// extension -> (count, size, alloc) of every file seen so far.
    types: Mutex<HashMap<String, (u64, u64, u64)>>,
    top: Mutex<Top>,
    /// Smallest size currently kept in the (full) largest list: smaller files are not offered.
    largest_floor: AtomicU64,
    /// Newest mtime currently kept in the (full) oldest list: newer files are not offered.
    oldest_ceil: AtomicI64,
}

impl Default for Live {
    fn default() -> Self {
        Live {
            dirs: Mutex::default(),
            store: Mutex::default(),
            types: Mutex::default(),
            top: Mutex::default(),
            largest_floor: AtomicU64::new(0),
            oldest_ceil: AtomicI64::new(i64::MAX),
        }
    }
}

/// Size of the streamed "Top N" lists.
pub const TOP_N: usize = 100;

#[derive(Default)]
struct Top {
    largest: Vec<RankedFile>,
    oldest: Vec<RankedFile>,
}

fn trim_largest(v: &mut Vec<RankedFile>) {
    v.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    v.truncate(TOP_N);
}

fn trim_oldest(v: &mut Vec<RankedFile>) {
    v.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.name.cmp(&b.name)));
    v.truncate(TOP_N);
}

/// Lower-case extension of `name` without the dot, written into `buf` (empty when there is none).
/// Same result as `analysis::types::ext_of`, without allocating.
fn ext_lower<'a>(name: &str, buf: &'a mut String) -> &'a str {
    buf.clear();
    if let Some(i) = name.rfind('.') {
        if i > 0 && i + 1 < name.len() {
            buf.push_str(&name[i + 1..]);
            buf.make_ascii_lowercase();
        }
    }
    buf.as_str()
}

/// Add every directory total to its parent, from the last record to the first (children always
/// have a larger index than their parent).
fn aggregate(nodes: &mut [Node]) {
    for i in (1..nodes.len()).rev() {
        let (size, alloc, files, dirs, parent) = {
            let n = &nodes[i];
            (n.size, n.alloc, n.files, n.dirs, n.parent as usize)
        };
        let p = &mut nodes[parent];
        p.size += size;
        p.alloc += alloc;
        p.files += files;
        p.dirs += 1 + dirs;
    }
}

impl Live {
    /// Register the sub-directories of `parent` (all of them in one call: their ids are consecutive)
    /// and return the id of the first one. `NONE` as parent registers the root.
    fn add_dirs<'a>(&self, parent: NodeId, items: impl IntoIterator<Item = (Cow<'a, str>, i64, u32)>) -> NodeId {
        let mut guard = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        let d = &mut *guard;
        let first = d.nodes.len() as NodeId;
        for (name, modified, attrs) in items {
            let node = Node {
                modified,
                attrs,
                scanning: true,
                name_off: d.names.len() as u32,
                name_len: name.len() as u32,
                ..Node::blank(parent, true)
            };
            d.names.push_str(&name);
            d.nodes.push(node);
            d.files.push([0, 0]);
        }
        if parent != NONE {
            let count = d.nodes.len() as NodeId - first;
            let p = &mut d.nodes[parent as usize];
            debug_assert_eq!(p.child_len, 0, "sub-directories are registered in one call");
            p.child_start = first;
            p.child_len = count;
        }
        first
    }

    fn set_direct(&self, id: NodeId, size: u64, alloc: u64, files: u64, dir_links: u64) {
        let mut d = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        let n = &mut d.nodes[id as usize];
        (n.size, n.alloc, n.files, n.dirs) = (size, alloc, files, dir_links);
    }

    /// Append the file and link entries of directory `lid` (names are offsets into `names`) to the
    /// file store, in one short lock, and remember their range on the directory.
    fn push_files(&self, lid: NodeId, entries: Vec<Node>, names: &str) {
        let (first, count) = {
            let mut guard = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let s = &mut *guard;
            let (first, base) = (s.total, s.names.len() as u32);
            s.names.push_str(names);
            let count = entries.len() as u32;
            for mut n in entries {
                n.name_off += base;
                if s.chunks.last().is_none_or(|c| c.len() == CHUNK) {
                    s.chunks.push(Vec::with_capacity(CHUNK));
                }
                if let Some(c) = s.chunks.last_mut() {
                    c.push(n);
                }
            }
            s.total += count;
            (first, count)
        };
        self.dirs.lock().unwrap_or_else(|e| e.into_inner()).files[lid as usize] = [first, count];
    }

    /// Fold the files of one directory into the live aggregates and the bounded top lists.
    /// `entries` are the directory's records, `names` their (batch-local) name buffer.
    fn record_files(&self, lid: NodeId, entries: &[Node], names: &str) {
        let name_of = |f: &Node| &names[f.name_off as usize..(f.name_off + f.name_len) as usize];
        let mut local: HashMap<String, (u64, u64, u64)> = HashMap::new();
        let mut buf = String::new();
        let floor = self.largest_floor.load(Ordering::Relaxed);
        let ceil = self.oldest_ceil.load(Ordering::Relaxed);
        let (mut big, mut old): (Vec<&Node>, Vec<&Node>) = (Vec::new(), Vec::new());
        for f in entries.iter().filter(|f| !f.is_dir && !f.is_link) {
            let ext = ext_lower(name_of(f), &mut buf);
            let e = match local.get_mut(ext) {
                Some(e) => e,
                None => local.entry(ext.to_owned()).or_default(),
            };
            e.0 += 1;
            e.1 += f.size;
            e.2 += f.alloc;
            if f.size > floor || floor == 0 {
                big.push(f);
            }
            if f.modified != 0 && f.modified < ceil {
                old.push(f);
            }
        }
        big.sort_by_key(|f| std::cmp::Reverse(f.size));
        big.truncate(TOP_N);
        old.sort_by_key(|f| f.modified);
        old.truncate(TOP_N);
        let rank = |f: &&Node| RankedFile { dir: lid, name: name_of(f).to_owned(), size: f.size, modified: f.modified };
        {
            let mut t = self.types.lock().unwrap_or_else(|e| e.into_inner());
            for (k, v) in local {
                let e = t.entry(k).or_default();
                e.0 += v.0;
                e.1 += v.1;
                e.2 += v.2;
            }
        }
        if big.is_empty() && old.is_empty() {
            return;
        }
        let mut top = self.top.lock().unwrap_or_else(|e| e.into_inner());
        top.largest.extend(big.iter().map(rank));
        top.oldest.extend(old.iter().map(rank));
        if top.largest.len() > TOP_N * 4 {
            trim_largest(&mut top.largest);
            if top.largest.len() >= TOP_N {
                self.largest_floor.store(top.largest[TOP_N - 1].size, Ordering::Relaxed);
            }
        }
        if top.oldest.len() > TOP_N * 4 {
            trim_oldest(&mut top.oldest);
            if top.oldest.len() >= TOP_N {
                self.oldest_ceil.store(top.oldest[TOP_N - 1].modified, Ordering::Relaxed);
            }
        }
    }

    fn finish(&self, id: NodeId) {
        self.dirs.lock().unwrap_or_else(|e| e.into_inner()).nodes[id as usize].scanning = false;
    }

    /// Consistent snapshot: directories only, totals aggregated up to the root, unfinished
    /// directories flagged `scanning`. Sizes only ever grow from one snapshot to the next.
    fn snapshot(&self) -> Tree {
        // Under the lock: two plain buffer copies (fixed-size records and the name buffer).
        let (mut nodes, names) = {
            let d = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
            (d.nodes.clone(), d.names.clone())
        };
        // Turn each record's consecutive sub-directory id range into a child list.
        let mut kids: Vec<NodeId> = Vec::with_capacity(nodes.len().saturating_sub(1));
        for n in nodes.iter_mut() {
            let (first, len) = (n.child_start, n.child_len);
            n.child_start = kids.len() as u32;
            kids.extend(first..first + len);
        }
        let direct: Vec<DirectFiles> = nodes.iter().map(|n| DirectFiles { count: n.files, size: n.size, alloc: n.alloc, modified: 0 }).collect();
        aggregate(&mut nodes);
        let mut types: Vec<ExtAgg> = {
            let t = self.types.lock().unwrap_or_else(|e| e.into_inner());
            t.iter().map(|(k, v)| ExtAgg { ext: k.clone(), count: v.0, size: v.1, alloc: v.2 }).collect()
        };
        types.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.ext.cmp(&b.ext)));
        let (mut largest, mut oldest) = {
            let t = self.top.lock().unwrap_or_else(|e| e.into_inner());
            (t.largest.clone(), t.oldest.clone())
        };
        trim_largest(&mut largest);
        trim_oldest(&mut oldest);
        Tree { nodes, names, kids, live: Some(Box::new(LiveStats { direct, types, largest, oldest })) }
    }

    /// Consume the stores into the final tree, numbered in pre-order (a directory, then its
    /// non-directory entries, then each sub-directory with its own subtree), every list in the order
    /// the entries were read. The numbering only depends on the folder contents, never on which
    /// thread finished first.
    fn into_tree(self) -> Tree {
        let LiveDirs { nodes: mut dirs, names: dir_names, files: ranges } = self.dirs.into_inner().unwrap_or_else(|e| e.into_inner());
        let FileStore { chunks, names: file_names, total } = self.store.into_inner().unwrap_or_else(|e| e.into_inner());
        let d = dirs.len();
        aggregate(&mut dirs);
        // Pre-order position of every directory: subtree sizes bottom-up (children have larger
        // live ids), then positions top-down.
        let mut size: Vec<u32> = ranges.iter().map(|r| 1 + r[1]).collect();
        for i in (1..d).rev() {
            let p = dirs[i].parent as usize;
            size[p] += size[i];
        }
        let mut pos = vec![0u32; d];
        for (i, n) in dirs.iter().enumerate() {
            let mut next = pos[i] + 1 + ranges[i][1];
            for c in n.child_start..n.child_start + n.child_len {
                pos[c as usize] = next;
                next += size[c as usize];
            }
        }
        drop(size);
        let mut tree = Tree::with_capacity(0, dir_names.len() + file_names.len(), d.saturating_sub(1) + total as usize);
        tree.nodes = vec![Node::blank(NONE, false); d + total as usize];
        let file_base = dir_names.len() as u32;
        tree.names.push_str(&dir_names);
        drop(dir_names);
        tree.names.push_str(&file_names);
        drop(file_names);
        // Files, in store order (each chunk is freed once placed).
        let mut g = 0u32;
        for chunk in chunks {
            for n in chunk {
                let p = n.parent as usize;
                tree.nodes[(pos[p] + 1 + g - ranges[p][0]) as usize] = Node { parent: pos[p], name_off: n.name_off + file_base, ..n };
                g += 1;
            }
        }
        for (i, mut n) in dirs.into_iter().enumerate() {
            let start = tree.kids.len() as u32;
            tree.kids.extend(pos[i] + 1..pos[i] + 1 + ranges[i][1]);
            tree.kids.extend((n.child_start..n.child_start + n.child_len).map(|c| pos[c as usize]));
            n.child_len += ranges[i][1];
            n.child_start = start;
            n.scanning = false;
            if n.parent != NONE {
                n.parent = pos[n.parent as usize];
            }
            tree.nodes[pos[i] as usize] = n;
        }
        tree
    }
}

/// Receives partial snapshots while a scan runs.
pub type Sink<'a> = &'a (dyn Fn(Tree) + Sync);

const FIRST_SNAPSHOT: Duration = Duration::from_millis(150);
const MIN_SNAPSHOT_GAP: Duration = Duration::from_millis(200);

struct Ctx<'a> {
    live: &'a Live,
    progress: &'a Progress,
    cancel: &'a AtomicBool,
    excludes: &'a [PathBuf],
    errors: Mutex<Vec<ScanError>>,
}

impl Ctx<'_> {
    fn err(&self, path: &Path, e: &std::io::Error) {
        self.progress.errors.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.errors.lock() {
            if v.len() < 10_000 {
                v.push(ScanError { path: path.to_path_buf(), message: e.to_string() });
            }
        }
    }
}

fn mtime(meta: &fs::Metadata) -> i64 {
    match meta.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        },
        Err(_) => 0,
    }
}

#[cfg(windows)]
fn attrs_of(meta: &fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes()
}
#[cfg(not(windows))]
fn attrs_of(_meta: &fs::Metadata) -> u32 {
    0
}

/// Allocated size on disk. Windows: GetCompressedFileSize for compressed/sparse
/// files, otherwise logical size rounded up to a 4 KiB cluster. Unix: st_blocks.
#[cfg(windows)]
fn allocated(entry: &fs::DirEntry, _meta: &fs::Metadata, logical: u64, attrs: u32) -> u64 {
    if logical == 0 {
        return 0;
    }
    if attrs & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE) != 0 {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = entry.path().as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut high: u32 = 0;
        // SAFETY: `wide` is a valid NUL-terminated UTF-16 string; `high` is a valid out pointer.
        let low = unsafe { windows_sys::Win32::Storage::FileSystem::GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
        // SAFETY: plain thread-local error query.
        if low != u32::MAX || unsafe { windows_sys::Win32::Foundation::GetLastError() } == 0 {
            return ((high as u64) << 32) | low as u64;
        }
    }
    logical.div_ceil(CLUSTER) * CLUSTER
}
#[cfg(not(windows))]
fn allocated(_entry: &fs::DirEntry, meta: &fs::Metadata, _logical: u64, _attrs: u32) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

/// Read one directory: its files and links go to the live file store as one batch, its
/// sub-directories are registered at once and walked in parallel. Totals are derived later from the
/// live records, so nothing is returned.
fn walk(path: &Path, ctx: &Ctx, lid: NodeId) {
    if ctx.cancel.load(Ordering::Relaxed) {
        return;
    }
    ctx.progress.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut cur) = ctx.progress.current.try_lock() {
        cur.clear();
        cur.push_str(&path.to_string_lossy());
    }
    let rd = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            ctx.err(path, &e);
            ctx.live.finish(lid);
            return;
        }
    };
    // Sub-directories to descend into: (path, mtime, attrs).
    let mut subs: Vec<(PathBuf, i64, u32)> = Vec::new();
    // This directory's file and link entries, with batch-local name offsets.
    let mut batch: Vec<Node> = Vec::new();
    let mut batch_names = String::new();
    let (mut d_size, mut d_alloc, mut d_files, mut d_links) = (0u64, 0u64, 0u64, 0u64);
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                ctx.err(path, &e);
                continue;
            }
        };
        if !ctx.excludes.is_empty() {
            let epath = entry.path();
            if ctx.excludes.iter().any(|x| x == &epath) {
                continue;
            }
        }
        // `DirEntry::metadata` does not follow links (and is free on Windows).
        let emeta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                ctx.err(&entry.path(), &e);
                continue;
            }
        };
        let attrs = attrs_of(&emeta);
        let ft = emeta.file_type();
        let is_link = ft.is_symlink() || (attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && ft.is_dir());
        if !is_link && ft.is_dir() {
            subs.push((entry.path(), mtime(&emeta), attrs));
            continue;
        }
        let (size, alloc) = if is_link {
            // Symlinks / junctions: record the entry, never descend (no loops).
            if ft.is_dir() {
                d_links += 1;
            } else {
                d_files += 1;
            }
            (0, 0)
        } else {
            let logical = emeta.len();
            let alloc = allocated(&entry, &emeta, logical, attrs);
            ctx.progress.files.fetch_add(1, Ordering::Relaxed);
            ctx.progress.bytes.fetch_add(logical, Ordering::Relaxed);
            d_size += logical;
            d_alloc += alloc;
            d_files += 1;
            (logical, alloc)
        };
        let ename = entry.file_name();
        let ename = ename.to_string_lossy();
        batch.push(Node {
            size,
            alloc,
            modified: mtime(&emeta),
            attrs,
            is_link,
            name_off: batch_names.len() as u32,
            name_len: ename.len() as u32,
            ..Node::blank(lid, ft.is_dir())
        });
        batch_names.push_str(&ename);
    }
    // Registered before they are read so the folders show up in the live tree at once.
    let first_sub = if subs.is_empty() {
        NONE
    } else {
        ctx.live.add_dirs(
            lid,
            subs.iter().map(|(p, modified, attrs)| (p.file_name().map_or(Cow::Borrowed(""), |n| n.to_string_lossy()), *modified, *attrs)),
        )
    };
    ctx.live.record_files(lid, &batch, &batch_names);
    ctx.live.push_files(lid, batch, &batch_names);
    drop(batch_names);
    ctx.live.set_direct(lid, d_size, d_alloc, d_files, d_links);
    subs.par_iter().enumerate().for_each(|(i, (p, _, _))| walk(p, ctx, first_sub + i as NodeId));
    ctx.live.finish(lid);
}

/// Scan `root` synchronously (blocks the calling thread). Cancel via `cancel`.
pub fn scan_blocking(root: &Path, progress: &Progress, cancel: &AtomicBool, excludes: &[PathBuf]) -> ScanResult {
    scan_live(root, progress, cancel, excludes, None)
}

/// Like `scan_blocking`; with a `sink`, partial snapshots (directories only, aggregated totals,
/// unfinished directories flagged `scanning`) are delivered every ~200 ms from a helper thread,
/// plus a last one when the walk ends. The sink must never block the scan.
pub fn scan_live(root: &Path, progress: &Progress, cancel: &AtomicBool, excludes: &[PathBuf], sink: Option<Sink>) -> ScanResult {
    let start = Instant::now();
    let live = Live::default();
    let ctx = Ctx { live: &live, progress, cancel, excludes, errors: Mutex::new(Vec::new()) };
    let root_name = root.to_string_lossy();
    let meta = fs::metadata(root).ok();
    let rid = live.add_dirs(NONE, [(root_name, meta.as_ref().map(mtime).unwrap_or(0), meta.as_ref().map(attrs_of).unwrap_or(0))]);
    let pool = rayon::ThreadPoolBuilder::new().stack_size(32 << 20).build();
    let finished = AtomicBool::new(false);
    std::thread::scope(|s| {
        if let Some(sink) = sink {
            s.spawn(|| {
                let mut wait = FIRST_SNAPSHOT;
                'outer: loop {
                    let until = Instant::now() + wait;
                    while Instant::now() < until {
                        if finished.load(Ordering::Relaxed) {
                            break 'outer;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    let t = Instant::now();
                    sink(live.snapshot());
                    // Back off when snapshots get expensive so they never steal the scan's time.
                    wait = MIN_SNAPSHOT_GAP.max(t.elapsed() * 4);
                }
            });
        }
        match &pool {
            Ok(p) => p.install(|| walk(root, &ctx, rid)),
            Err(_) => walk(root, &ctx, rid),
        }
        finished.store(true, Ordering::Relaxed);
    });
    if let Some(sink) = sink {
        sink(live.snapshot());
    }
    let errors = ctx.errors.into_inner().unwrap_or_default();
    ScanResult { root: root.to_path_buf(), tree: live.into_tree(), errors, cancelled: cancel.load(Ordering::Relaxed), elapsed: start.elapsed() }
}

/// Convenience used by the CLI and tests: scan without progress/cancel plumbing.
pub fn scan_simple(root: &Path) -> ScanResult {
    scan_blocking(root, &Progress::default(), &AtomicBool::new(false), &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Register one directory below `parent` (tests only: a parent may get a single such call).
    fn add(live: &Live, name: &str, parent: NodeId, modified: i64, attrs: u32) -> NodeId {
        live.add_dirs(parent, [(Cow::Borrowed(name), modified, attrs)])
    }

    #[test]
    fn snapshot_aggregates_and_flags_unfinished_dirs() {
        let live = Live::default();
        let root = add(&live, "R", NONE, 0, 0);
        let a = add(&live, "a", root, 0, 0);
        let b = add(&live, "b", a, 0, 0);
        live.set_direct(root, 1, 4096, 1, 0);
        live.set_direct(a, 10, 4096, 2, 0);
        live.set_direct(b, 100, 4096, 3, 1);
        live.finish(root);
        live.finish(b);
        let t = live.snapshot();
        let n = |i: NodeId| t.get(i);
        assert_eq!((n(root).size, n(root).files, n(root).dirs), (111, 6, 3));
        assert_eq!((n(a).size, n(a).files, n(a).dirs), (110, 5, 2));
        assert_eq!((n(b).size, n(b).files, n(b).dirs), (100, 3, 1));
        assert!(!n(root).scanning && n(a).scanning && !n(b).scanning);
        // growing a directory later never shrinks any total
        live.set_direct(b, 150, 4096, 4, 1);
        live.finish(a);
        let t2 = live.snapshot();
        assert!(t2.get(root).size >= t.get(root).size && !t2.get(a).scanning);
        // direct files are reported per directory, before aggregation
        let l = t2.live.as_ref().unwrap();
        assert_eq!((l.direct[root as usize].count, l.direct[root as usize].size), (1, 1));
        assert_eq!((l.direct[b as usize].count, l.direct[b as usize].size), (4, 150));
    }

    /// A batch of plain files (name, size, modified) as the walker builds it: records plus names.
    fn batch(files: &[(String, u64, i64)]) -> (Vec<Node>, String) {
        let mut names = String::new();
        let nodes = files
            .iter()
            .map(|(name, size, modified)| {
                let node = Node { size: *size, alloc: *size, modified: *modified, name_off: names.len() as u32, name_len: name.len() as u32, ..Node::blank(0, false) };
                names.push_str(name);
                node
            })
            .collect();
        (nodes, names)
    }

    fn file(name: &str, size: u64, modified: i64) -> (String, u64, i64) {
        (name.to_string(), size, modified)
    }

    #[test]
    fn live_stats_aggregate_and_stay_bounded() {
        let live = Live::default();
        let root = add(&live, "R", NONE, 0, 0);
        // 3 batches of 150 files each: far more than TOP_N in total
        for b in 0..3u64 {
            let files: Vec<_> = (0..150u64).map(|i| file(&format!("f{b}_{i}.dat"), b * 1000 + i, 1000 + (b * 150 + i) as i64)).collect();
            let (nodes, names) = batch(&files);
            live.record_files(root, &nodes, &names);
        }
        let (nodes, names) = batch(&[file("Readme.TXT", 5, 1), file("noext", 7, 2), file("junction.dat", 0, 3)]);
        live.record_files(root, &nodes, &names);
        live.set_direct(root, 0, 0, 0, 0);
        let t = live.snapshot();
        let l = t.live.as_ref().unwrap();
        assert_eq!(l.largest.len(), TOP_N);
        assert_eq!(l.oldest.len(), TOP_N);
        assert!(l.largest.windows(2).all(|w| w[0].size >= w[1].size), "largest sorted descending");
        assert!(l.oldest.windows(2).all(|w| w[0].modified <= w[1].modified), "oldest sorted ascending");
        assert_eq!(l.largest[0].size, 2149);
        assert_eq!(l.oldest[0].modified, 1);
        let dat = l.types.iter().find(|e| e.ext == "dat").unwrap();
        assert_eq!(dat.count, 451);
        assert!(l.types.iter().any(|e| e.ext == "txt" && e.count == 1), "extensions are lower-cased");
        assert!(l.types.iter().any(|e| e.ext.is_empty() && e.size == 7));
        assert_eq!(l.types.iter().map(|e| e.count).sum::<u64>(), 453);
        assert!(l.types.windows(2).all(|w| w[0].size >= w[1].size));
    }

    #[test]
    fn ext_lower_matches_ext_of() {
        let mut buf = String::new();
        for name in ["Photo.JPG", "README", ".gitignore", "trailing.", "a.b.TAR.GZ", "x.", ".", "", "\u{e9}t\u{e9}.D\u{c9}J", "dir.name/f"] {
            assert_eq!(ext_lower(name, &mut buf), crate::analysis::types::ext_of(name), "{name:?}");
        }
    }

    // ---- whole-scan tests on temporary folders ----

    use std::fs::File;
    use std::io::Write;

    fn put(dir: &Path, name: &str, size: usize) {
        File::create(dir.join(name)).unwrap().write_all(&vec![b'x'; size]).unwrap();
    }

    /// Directory link without admin rights when the platform allows it; false when it cannot be made.
    fn make_dir_link(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return true;
            }
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            false
        }
    }

    /// Expected tree lines `rel|kind|size|files|dirs` (pre-order, files and links before
    /// sub-directories, read_dir order), computed with plain std::fs.
    fn expected(dir: &Path, rel: &str) -> (Vec<String>, u64, u64, u64) {
        let (mut flat, mut subs) = (Vec::new(), Vec::new());
        let (mut size, mut files, mut dirs) = (0u64, 0u64, 0u64);
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let meta = e.metadata().unwrap();
            let ft = meta.file_type();
            let is_link = ft.is_symlink() || (attrs_of(&meta) & FILE_ATTRIBUTE_REPARSE_POINT != 0 && ft.is_dir());
            if is_link {
                if ft.is_dir() {
                    dirs += 1;
                } else {
                    files += 1;
                }
                flat.push(format!("{child}|L|0|0|0"));
            } else if ft.is_dir() {
                subs.push((e.path(), child));
            } else {
                size += meta.len();
                files += 1;
                flat.push(format!("{child}|F|{}|0|0", meta.len()));
            }
        }
        let mut lines = Vec::new();
        for (p, child) in subs {
            let (l, s, f, d) = expected(&p, &child);
            size += s;
            files += f;
            dirs += 1 + d;
            lines.extend(l);
        }
        let mut out = vec![format!("{rel}|D|{size}|{files}|{dirs}")];
        out.extend(flat);
        out.extend(lines);
        (out, size, files, dirs)
    }

    fn lines_of(t: &Tree, id: NodeId, rel: &str, out: &mut Vec<String>) {
        let n = t.get(id);
        let kind = if n.is_link { "L" } else if n.is_dir { "D" } else { "F" };
        out.push(format!("{rel}|{kind}|{}|{}|{}", n.size, n.files, n.dirs));
        for &c in t.children(id) {
            let child = if rel.is_empty() { t.name(c).to_string() } else { format!("{rel}/{}", t.name(c)) };
            lines_of(t, c, &child, out);
        }
    }

    /// Structural invariants of a finished tree.
    fn check_tree(t: &Tree) {
        for (i, n) in t.nodes.iter().enumerate() {
            let id = i as NodeId;
            assert!(!n.scanning, "node {id} still flagged scanning");
            assert!(n.is_dir || t.children(id).is_empty());
            let (mut size, mut files, mut dirs, mut seen_dir) = (0u64, 0u64, 0u64, false);
            for &c in t.children(id) {
                let ch = t.get(c);
                assert!(c > id && ch.parent == id, "child {c} of {id}");
                let sub = ch.is_dir && !ch.is_link;
                assert!(!(seen_dir && !sub), "files must come before sub-directories");
                seen_dir |= sub;
                size += ch.size;
                if ch.is_dir {
                    dirs += 1 + ch.dirs;
                    files += ch.files;
                } else {
                    files += 1;
                }
            }
            if n.is_dir {
                assert_eq!((n.size, n.files, n.dirs), (size, files, dirs), "totals of node {id}");
            }
        }
    }

    #[test]
    fn scan_matches_independent_walk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(root, "top.TXT", 10);
        put(root, "zero.bin", 0);
        put(root, "r\u{e9}sum\u{e9}.pdf", 5000);
        fs::create_dir(root.join("empty")).unwrap();
        fs::create_dir_all(root.join("\u{65e5}\u{672c}\u{8a9e}/deep/deeper")).unwrap();
        put(&root.join("\u{65e5}\u{672c}\u{8a9e}"), "\u{fc}n\u{ef}.dat", 123);
        put(&root.join("\u{65e5}\u{672c}\u{8a9e}/deep"), "emoji-\u{1f600}.log", 77);
        put(&root.join("\u{65e5}\u{672c}\u{8a9e}/deep/deeper"), "a.b.c", 1);
        fs::create_dir_all(root.join("src/empty_inner")).unwrap();
        put(&root.join("src"), "main.rs", 4097);
        let linked = make_dir_link(&root.join("src"), &root.join("a_link"));
        eprintln!("directory link created: {linked}");
        let (want, ..) = expected(root, "");
        let res = scan_simple(root);
        assert!(!res.cancelled && res.errors.is_empty());
        let mut got = Vec::new();
        lines_of(&res.tree, Tree::ROOT, "", &mut got);
        assert_eq!(got, want);
        check_tree(&res.tree);
        if linked {
            assert!(got.iter().any(|l| l.starts_with("a_link|L|")), "link listed but not followed");
            assert!(!got.iter().any(|l| l.starts_with("a_link/")));
        }
        assert_eq!(res.tree.nodes.len(), got.len());
        // ids are a pre-order numbering: stable whatever thread finished first
        fn preorder(t: &Tree, id: NodeId, out: &mut Vec<NodeId>) {
            out.push(id);
            for &c in t.children(id) {
                preorder(t, c, out);
            }
        }
        let mut ids = Vec::new();
        preorder(&res.tree, Tree::ROOT, &mut ids);
        assert!(ids.iter().enumerate().all(|(i, &id)| id as usize == i));
    }

    #[test]
    fn cancelled_scan_returns_consistent_tree() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..200 {
            let d = tmp.path().join(format!("d{i}"));
            fs::create_dir_all(d.join("inner")).unwrap();
            for j in 0..10 {
                put(&d, &format!("f{j}.dat"), 100 + j);
                put(&d.join("inner"), &format!("g{j}.txt"), 7);
            }
        }
        let progress = Progress::default();
        let cancel = AtomicBool::new(false);
        let done = AtomicBool::new(false);
        let res = std::thread::scope(|s| {
            s.spawn(|| {
                while !done.load(Ordering::Relaxed) && progress.dirs.load(Ordering::Relaxed) < 20 {
                    std::thread::yield_now();
                }
                cancel.store(true, Ordering::Relaxed);
            });
            let r = scan_blocking(tmp.path(), &progress, &cancel, &[]);
            done.store(true, Ordering::Relaxed);
            r
        });
        assert!(res.cancelled);
        check_tree(&res.tree);
        let full = 200 * 20u64;
        assert!(res.tree.get(Tree::ROOT).files <= full);
        // every name resolves and parents/children agree
        for id in 0..res.tree.nodes.len() as NodeId {
            let _ = res.tree.name(id);
            assert!(res.tree.path_of(id).starts_with(tmp.path()));
        }
    }

    #[test]
    fn snapshots_during_a_scan_stay_ordered_and_monotonic() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..40 {
            let d = tmp.path().join(format!("d{i}"));
            fs::create_dir_all(d.join("sub")).unwrap();
            for j in 0..40 {
                put(&d, &format!("f{j}.dat"), 10 + j);
            }
            for j in 0..25 {
                put(&d.join("sub"), &format!("g{j}.txt"), 3);
            }
        }
        // 40 * (40 + 25) = 2600 files
        let live = Live::default();
        let progress = Progress::default();
        let cancel = AtomicBool::new(false);
        let ctx = Ctx { live: &live, progress: &progress, cancel: &cancel, excludes: &[], errors: Mutex::new(Vec::new()) };
        let name = tmp.path().to_string_lossy();
        let rid = live.add_dirs(NONE, [(name, 0, 0)]);
        let finished = AtomicBool::new(false);
        let snaps = std::thread::scope(|s| {
            let h = s.spawn(|| {
                let mut v = Vec::new();
                while !finished.load(Ordering::Relaxed) {
                    v.push(live.snapshot());
                    std::thread::yield_now();
                }
                v.push(live.snapshot());
                v
            });
            walk(tmp.path(), &ctx, rid);
            finished.store(true, Ordering::Relaxed);
            h.join().unwrap()
        });
        assert!(!snaps.is_empty());
        let (mut size, mut files, mut dirs) = (0u64, 0u64, 0u64);
        for t in &snaps {
            for (i, n) in t.nodes.iter().enumerate() {
                for &c in t.children(i as NodeId) {
                    assert!(c as usize > i, "child {c} of {i}");
                    assert_eq!(t.get(c).parent as usize, i);
                }
                assert!(n.is_dir);
            }
            let r = t.get(Tree::ROOT);
            assert!(r.size >= size && r.files >= files && r.dirs >= dirs, "totals shrank");
            (size, files, dirs) = (r.size, r.files, r.dirs);
        }
        let last = snaps.last().unwrap();
        assert_eq!(last.get(Tree::ROOT).files, 2600);
        assert_eq!(last.get(Tree::ROOT).dirs, 80);
        assert!(last.nodes.iter().all(|n| !n.scanning));
        assert_eq!(live.into_tree().get(Tree::ROOT).files, 2600);
    }
}
