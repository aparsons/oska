// SPDX-License-Identifier: Apache-2.0
//! A filesystem that crashes (#847, spec #842): [`CrashHost`] runs a
//! program on a [`FakeHost`] and records every step the program takes, and
//! [`CrashRecording::for_each_crash`] then hands a recovery callback the
//! filesystem as a crash after each step may have left it.
//!
//! The rules are the crash rules of `formal/tla/store-log.md`:
//!
//! - **Data.** `write` and `append` go to a cache. A file's `sync` makes
//!   its data durable. A crash keeps the durable data, then any first few of
//!   the unsynced writes (in the order the program made them, across all
//!   files), and the next one may be torn: only a prefix of its bytes is
//!   there.
//! - **Names.** A `rename`, `remove_file` or `remove_dir` is visible at
//!   once, but durable only when the directory it changes is synced
//!   (`DirWrite.sync`; a rename across two directories needs both). A crash
//!   keeps the durable names and any first few of the unsynced changes, so
//!   an unsynced rename may or may not have happened. Data and names are
//!   chosen independently, so a rename may survive while the data of the
//!   file it moved does not: a torn snapshot.
//! - **Created names** (a new file, `create_dir`) are durable at once, as
//!   the model has no creation to lose. A new file's data is not.
//! - **Acknowledged.** The recovery callback is told what the program had
//!   printed when it crashed, which is what a program acknowledges with.
//!   Every print is a crash point of its own, so a program that prints
//!   before it syncs is caught between the two.
//!
//! The program runs once; each crash point is a replay of its recording, so
//! the program need not be deterministic past the first run and its crash
//! points cost no re-run.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::host::{joined, DirLock, FakeHost, FakeWorld, Host, Meta, Output, RootHandle};

/// One step of a program that a crash can follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A write to stdout, as the program gave it.
    Print(String),
    /// `write`: the file is created or its contents replaced.
    Write { path: PathBuf, bytes: Vec<u8> },
    /// `append`: the file is created if missing, bytes added at its end.
    Append { path: PathBuf, bytes: Vec<u8> },
    /// `create_dir`.
    CreateDir(PathBuf),
    /// `remove_file`.
    Remove(PathBuf),
    /// `remove_dir`.
    RemoveDir(PathBuf),
    /// `rename`.
    Rename { from: PathBuf, to: PathBuf },
    /// A file's `sync`.
    SyncFile(PathBuf),
    /// A directory's `sync`.
    SyncDir(PathBuf),
    /// `with_lock` made its lock file if it was missing.
    Lock(PathBuf),
}

impl Event {
    /// The step in a line, for a failure message.
    pub fn describe(&self) -> String {
        match self {
            Event::Print(text) => format!("print {:?}", text.trim_end_matches('\n')),
            Event::Write { path, bytes } => {
                format!("write {} ({} bytes)", path.display(), bytes.len())
            }
            Event::Append { path, bytes } => {
                format!("append {} ({} bytes)", path.display(), bytes.len())
            }
            Event::CreateDir(path) => format!("create_dir {}", path.display()),
            Event::Remove(path) => format!("remove_file {}", path.display()),
            Event::RemoveDir(path) => format!("remove_dir {}", path.display()),
            Event::Rename { from, to } => {
                format!("rename {} to {}", from.display(), to.display())
            }
            Event::SyncFile(path) => format!("sync file {}", path.display()),
            Event::SyncDir(path) => format!("sync dir {}", path.display()),
            Event::Lock(path) => format!("lock {}", path.display()),
        }
    }
}

/// Which prefixes of the last unsynced write a crash may leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tears {
    /// None: a write is there whole or not at all.
    None,
    /// Half of its bytes (rounded down), when that is neither none nor all.
    Half,
    /// Every proper prefix, one to one fewer than all its bytes.
    Every,
}

impl Tears {
    fn lengths(self, len: usize) -> Vec<usize> {
        match self {
            Tears::None => Vec::new(),
            Tears::Half if len >= 2 => vec![len / 2],
            Tears::Half => Vec::new(),
            Tears::Every => (1..len).collect(),
        }
    }
}

/// A [`FakeHost`] that records what the program does to its filesystem and
/// stdout. Run the program on it, then take [`CrashHost::into_recording`].
#[derive(Debug)]
pub struct CrashHost {
    fs: FakeHost,
    initial: FakeWorld,
    events: Vec<Event>,
}

impl CrashHost {
    /// A crash host over `fs`, whose files and directories are all
    /// durable to begin with.
    pub fn new(fs: FakeHost) -> Self {
        CrashHost {
            initial: fs.world(),
            fs,
            events: Vec::new(),
        }
    }

    /// The steps so far.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The filesystem as the program sees it now, with nothing lost.
    pub fn fs(&self) -> &FakeHost {
        &self.fs
    }

    /// What the run did, ready to enumerate its crashes.
    pub fn into_recording(self) -> CrashRecording {
        CrashRecording {
            initial: self.initial,
            events: self.events,
        }
    }

    fn note(&mut self, event: Event) {
        self.events.push(event);
    }
}

impl Host for CrashHost {
    fn write_stdout(&mut self, text: &str) -> io::Result<()> {
        self.fs.write_stdout(text)?;
        self.note(Event::Print(text.to_string()));
        Ok(())
    }

    fn write_stderr(&mut self, text: &str) -> io::Result<()> {
        self.fs.write_stderr(text)
    }

    fn read_line(&mut self) -> io::Result<Option<String>> {
        self.fs.read_line()
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        self.fs.env_var(name)
    }

    fn now_millis(&mut self) -> i128 {
        self.fs.now_millis()
    }

    fn next_random(&mut self) -> u64 {
        self.fs.next_random()
    }

    fn secure_bytes(&mut self, len: usize) -> io::Result<Vec<u8>> {
        self.fs.secure_bytes(len)
    }

    fn open_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        self.fs.open_root(path)
    }

    fn open_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        self.fs.open_beneath(root, names)
    }

    fn open_file_root(&mut self, path: &Path) -> io::Result<RootHandle> {
        self.fs.open_file_root(path)
    }

    fn open_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<RootHandle> {
        self.fs.open_file_beneath(root, names)
    }

    fn read_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<u8>> {
        self.fs.read_file_beneath(root, names)
    }

    fn write_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        self.fs.write_file_beneath(root, names, contents)?;
        self.note(Event::Write {
            path: joined(root, names),
            bytes: contents.to_vec(),
        });
        Ok(())
    }

    fn create_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        self.fs.create_dir_beneath(root, names)?;
        self.note(Event::CreateDir(joined(root, names)));
        Ok(())
    }

    fn lock_beneath(&mut self, root: &RootHandle) -> io::Result<DirLock> {
        let lock = self.fs.lock_beneath(root)?;
        self.note(Event::Lock(root.path().join(oska_sys::LOCK_NAME)));
        Ok(lock)
    }

    fn list_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Vec<String>> {
        self.fs.list_dir_beneath(root, names)
    }

    fn exists_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<bool> {
        self.fs.exists_beneath(root, names)
    }

    fn metadata_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<Meta> {
        self.fs.metadata_beneath(root, names)
    }

    fn append_file_beneath(
        &mut self,
        root: &RootHandle,
        names: &[String],
        contents: &[u8],
    ) -> io::Result<()> {
        self.fs.append_file_beneath(root, names, contents)?;
        self.note(Event::Append {
            path: joined(root, names),
            bytes: contents.to_vec(),
        });
        Ok(())
    }

    fn sync_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        self.fs.sync_file_beneath(root, names)?;
        self.note(Event::SyncFile(joined(root, names)));
        Ok(())
    }

    fn sync_dir_beneath(&mut self, root: &RootHandle) -> io::Result<()> {
        self.fs.sync_dir_beneath(root)?;
        self.note(Event::SyncDir(root.path().to_path_buf()));
        Ok(())
    }

    fn remove_file_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        self.fs.remove_file_beneath(root, names)?;
        self.note(Event::Remove(joined(root, names)));
        Ok(())
    }

    fn remove_dir_beneath(&mut self, root: &RootHandle, names: &[String]) -> io::Result<()> {
        self.fs.remove_dir_beneath(root, names)?;
        self.note(Event::RemoveDir(joined(root, names)));
        Ok(())
    }

    fn rename_beneath(
        &mut self,
        root: &RootHandle,
        from: &[String],
        to: &[String],
    ) -> io::Result<()> {
        self.fs.rename_beneath(root, from, to)?;
        let (from, to) = (joined(root, from), joined(root, to));
        if from != to {
            self.note(Event::Rename { from, to });
        }
        Ok(())
    }

    fn net_connect(&mut self, host: &str, port: i128) -> io::Result<oska_sys::NetStream> {
        self.fs.net_connect(host, port)
    }

    fn net_listen_loopback(&mut self, port: i128) -> io::Result<oska_sys::NetListener> {
        self.fs.net_listen_loopback(port)
    }

    fn net_exchange_datagram(
        &mut self,
        host: &str,
        port: i128,
        message: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.fs.net_exchange_datagram(host, port, message)
    }

    fn net_name_servers(&mut self) -> io::Result<Vec<String>> {
        self.fs.net_name_servers()
    }

    fn net_attempts(&self) -> Vec<String> {
        self.fs.net_attempts()
    }

    fn syncs(&self) -> Vec<String> {
        self.fs.syncs()
    }

    fn printed(&self) -> Vec<String> {
        self.fs.printed()
    }

    fn take_output(&mut self) -> Output {
        self.fs.take_output()
    }
}

/// A run's steps and the world it started from.
#[derive(Debug, Clone)]
pub struct CrashRecording {
    initial: FakeWorld,
    events: Vec<Event>,
}

/// One way the machine may have stopped.
#[derive(Debug)]
pub struct Crash {
    /// How many steps of the run were done: 0 is a crash before the first,
    /// [`CrashRecording::events`]`.len()` one after the last.
    pub after: usize,
    /// The last step done, in words; `None` at 0.
    pub last_step: Option<String>,
    /// What the program had printed, a line each, as a user saw it.
    pub printed: Vec<String>,
    /// How many unsynced writes were kept whole, out of
    /// [`Crash::unsynced_writes`].
    pub writes_kept: usize,
    /// How many writes no sync had made durable.
    pub unsynced_writes: usize,
    /// How many bytes of the write after the kept ones are there, if it
    /// was torn.
    pub torn_to: Option<usize>,
    /// How many unsynced name changes (a rename or a remove) happened,
    /// out of [`Crash::unsynced_renames`].
    pub renames_kept: usize,
    /// How many name changes no directory sync had made durable.
    pub unsynced_renames: usize,
    /// The filesystem after the crash, to run recovery on.
    pub fs: FakeHost,
}

impl Crash {
    /// This crash in a line, for a failure message.
    pub fn describe(&self) -> String {
        format!(
            "crash after step {} ({}): kept {} of {} unsynced writes{}, {} of {} unsynced name changes",
            self.after,
            self.last_step.as_deref().unwrap_or("none"),
            self.writes_kept,
            self.unsynced_writes,
            self.torn_to
                .map_or(String::new(), |t| format!(", the next torn to {t} bytes")),
            self.renames_kept,
            self.unsynced_renames,
        )
    }
}

impl CrashRecording {
    /// Every step the run took, in order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Call `recover` with every crash the rules allow, after each step
    /// from none to all, and return how many that was. The same
    /// filesystem is not given twice for one step.
    pub fn for_each_crash(&self, tears: Tears, mut recover: impl FnMut(&Crash)) -> usize {
        let mut model = Model::new(&self.initial);
        let mut printed = Vec::new();
        let mut count = 0;
        for after in 0..=self.events.len() {
            if after > 0 {
                let event = &self.events[after - 1];
                model.apply(event);
                if let Event::Print(text) = event {
                    printed.push(text.strip_suffix('\n').unwrap_or(text).to_string());
                }
            }
            let last_step = after.checked_sub(1).map(|i| self.events[i].describe());
            count += model.crashes(&self.initial, tears, |mut crash| {
                crash.after = after;
                crash.last_step.clone_from(&last_step);
                crash.printed.clone_from(&printed);
                recover(&crash);
            });
        }
        count
    }
}

/// Run `run` on a [`CrashHost`] over `fs`, then call `recover` with every
/// crash of that run (see [`CrashRecording::for_each_crash`]). Returns how
/// many crashes `recover` saw.
pub fn explore(
    fs: FakeHost,
    tears: Tears,
    run: impl FnOnce(&mut CrashHost),
    recover: impl FnMut(&Crash),
) -> usize {
    let mut host = CrashHost::new(fs);
    run(&mut host);
    host.into_recording().for_each_crash(tears, recover)
}

// ---- The model -------------------------------------------------------------------------

/// A change of a name, durable once the directories it changes are synced.
#[derive(Debug, Clone)]
enum NameChange {
    Create(PathBuf, usize),
    Remove(PathBuf),
    RemoveDir(PathBuf),
    Rename(PathBuf, PathBuf),
}

impl NameChange {
    fn touches(&self, path: &Path) -> bool {
        match self {
            NameChange::Create(p, _) | NameChange::Remove(p) | NameChange::RemoveDir(p) => {
                p == path
            }
            NameChange::Rename(from, to) => from == path || to == path,
        }
    }

    fn apply(&self, names: &mut BTreeMap<PathBuf, usize>, dirs: &mut BTreeSet<PathBuf>) {
        match self {
            NameChange::Create(path, inode) => {
                names.insert(path.clone(), *inode);
            }
            NameChange::Remove(path) => {
                names.remove(path);
            }
            NameChange::RemoveDir(path) => {
                dirs.remove(path);
            }
            NameChange::Rename(from, to) => {
                if let Some(inode) = names.remove(from) {
                    names.insert(to.clone(), inode);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PendingName {
    change: NameChange,
    /// The directories whose sync it still waits for.
    awaiting: BTreeSet<PathBuf>,
}

/// Data a file's sync has not made durable.
#[derive(Debug, Clone)]
enum Data {
    Write(Vec<u8>),
    Append(Vec<u8>),
}

impl Data {
    fn bytes(&self) -> &[u8] {
        match self {
            Data::Write(b) | Data::Append(b) => b,
        }
    }

    /// The file's contents after this, given them before.
    fn apply(&self, before: &mut Vec<u8>) {
        match self {
            Data::Write(bytes) => before.clone_from(bytes),
            Data::Append(bytes) => before.extend_from_slice(bytes),
        }
    }

    /// The same step with only its first `len` bytes.
    fn torn(&self, len: usize) -> Data {
        match self {
            Data::Write(b) => Data::Write(b[..len].to_vec()),
            Data::Append(b) => Data::Append(b[..len].to_vec()),
        }
    }
}

/// A filesystem's files and directories, to tell two crashes apart.
type Shape = (BTreeMap<PathBuf, Vec<u8>>, BTreeSet<PathBuf>);

/// The state of the disk and of its cache after some steps.
#[derive(Debug, Clone)]
struct Model {
    /// The names the program sees.
    visible: BTreeMap<PathBuf, usize>,
    /// The names a crash is sure to keep.
    durable: BTreeMap<PathBuf, usize>,
    dirs: BTreeSet<PathBuf>,
    /// Each file's durable bytes, by inode.
    synced: Vec<Vec<u8>>,
    /// Unsynced data, in the order the program wrote it.
    unsynced: Vec<(usize, Data)>,
    /// Unsynced name changes, in order.
    pending: Vec<PendingName>,
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new("/")).to_path_buf()
}

impl Model {
    fn new(initial: &FakeWorld) -> Model {
        let mut model = Model {
            visible: BTreeMap::new(),
            durable: BTreeMap::new(),
            dirs: initial.dirs.clone(),
            synced: Vec::new(),
            unsynced: Vec::new(),
            pending: Vec::new(),
        };
        for (path, bytes) in &initial.files {
            model.synced.push(bytes.clone());
            let inode = model.synced.len() - 1;
            model.visible.insert(path.clone(), inode);
            model.durable.insert(path.clone(), inode);
            model.keep_ancestors(path);
        }
        model
    }

    fn keep_ancestors(&mut self, path: &Path) {
        for ancestor in path.ancestors().skip(1) {
            if ancestor != Path::new("/") && !ancestor.as_os_str().is_empty() {
                self.dirs.insert(ancestor.to_path_buf());
            }
        }
    }

    /// The inode at `path`, made if it was missing. The name is durable at
    /// once unless an unsynced change of that name came before.
    fn inode_at(&mut self, path: &Path) -> usize {
        if let Some(inode) = self.visible.get(path) {
            return *inode;
        }
        self.synced.push(Vec::new());
        let inode = self.synced.len() - 1;
        self.visible.insert(path.to_path_buf(), inode);
        self.keep_ancestors(path);
        let create = NameChange::Create(path.to_path_buf(), inode);
        if self.pending.iter().any(|p| p.change.touches(path)) {
            self.pending.push(PendingName {
                change: create,
                awaiting: BTreeSet::from([parent_of(path)]),
            });
        } else {
            create.apply(&mut self.durable, &mut self.dirs);
        }
        inode
    }

    fn change_name(&mut self, change: NameChange, dirs: &[PathBuf]) {
        self.pending.push(PendingName {
            change,
            awaiting: dirs.iter().cloned().collect(),
        });
    }

    fn apply(&mut self, event: &Event) {
        match event {
            Event::Print(_) => {}
            Event::Write { path, bytes } => {
                let inode = self.inode_at(path);
                self.unsynced.push((inode, Data::Write(bytes.clone())));
            }
            Event::Append { path, bytes } => {
                let inode = self.inode_at(path);
                self.unsynced.push((inode, Data::Append(bytes.clone())));
            }
            Event::Lock(path) => {
                self.inode_at(path);
            }
            Event::CreateDir(path) => {
                self.dirs.insert(path.clone());
                self.keep_ancestors(path);
            }
            Event::Remove(path) => {
                self.visible.remove(path);
                self.keep_ancestors(path);
                self.change_name(NameChange::Remove(path.clone()), &[parent_of(path)]);
            }
            Event::RemoveDir(path) => {
                self.change_name(NameChange::RemoveDir(path.clone()), &[parent_of(path)]);
            }
            Event::Rename { from, to } => {
                if let Some(inode) = self.visible.remove(from) {
                    self.visible.insert(to.clone(), inode);
                }
                self.keep_ancestors(from);
                self.keep_ancestors(to);
                self.change_name(
                    NameChange::Rename(from.clone(), to.clone()),
                    &[parent_of(from), parent_of(to)],
                );
            }
            Event::SyncFile(path) => {
                if let Some(&inode) = self.visible.get(path) {
                    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.unsynced)
                        .into_iter()
                        .partition(|(i, _)| *i == inode);
                    self.unsynced = rest;
                    for (_, data) in mine {
                        data.apply(&mut self.synced[inode]);
                    }
                }
            }
            Event::SyncDir(dir) => {
                for pending in &mut self.pending {
                    pending.awaiting.remove(dir);
                }
                while self.pending.first().is_some_and(|p| p.awaiting.is_empty()) {
                    let done = self.pending.remove(0);
                    done.change.apply(&mut self.durable, &mut self.dirs);
                }
            }
        }
    }

    /// Call `found` with each filesystem a crash now may leave, once each.
    /// Returns how many.
    fn crashes(&self, initial: &FakeWorld, tears: Tears, mut found: impl FnMut(Crash)) -> usize {
        let mut seen: BTreeSet<Shape> = BTreeSet::new();
        let mut count = 0;
        // The data choices: the first `kept` writes whole, and then the
        // next one torn to `torn` bytes, or not.
        let mut data_choices = Vec::new();
        for kept in 0..=self.unsynced.len() {
            data_choices.push((kept, None));
            if let Some((_, next)) = self.unsynced.get(kept) {
                for len in tears.lengths(next.bytes().len()) {
                    data_choices.push((kept, Some(len)));
                }
            }
        }
        for names_kept in 0..=self.pending.len() {
            let mut names = self.durable.clone();
            let mut dirs = self.dirs.clone();
            for pending in &self.pending[..names_kept] {
                pending.change.apply(&mut names, &mut dirs);
            }
            for &(kept, torn) in &data_choices {
                let mut bytes = self.synced.clone();
                for (inode, data) in &self.unsynced[..kept] {
                    data.apply(&mut bytes[*inode]);
                }
                if let (Some(len), Some((inode, next))) = (torn, self.unsynced.get(kept)) {
                    next.torn(len).apply(&mut bytes[*inode]);
                }
                let files: BTreeMap<PathBuf, Vec<u8>> = names
                    .iter()
                    .map(|(path, inode)| (path.clone(), bytes[*inode].clone()))
                    .collect();
                if !seen.insert((files.clone(), dirs.clone())) {
                    continue;
                }
                count += 1;
                found(Crash {
                    after: 0,
                    last_step: None,
                    printed: Vec::new(),
                    writes_kept: kept,
                    unsynced_writes: self.unsynced.len(),
                    torn_to: torn,
                    renames_kept: names_kept,
                    unsynced_renames: self.pending.len(),
                    fs: FakeHost::from_world(FakeWorld {
                        files,
                        dirs: dirs.clone(),
                        ..initial.clone()
                    }),
                });
            }
        }
        count
    }
}
