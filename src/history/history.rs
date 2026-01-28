//! Fish supports multiple shells writing to history at once. Here is its strategy:
//!
//! 1. All history files are append-only. Data, once written, is never modified.
//!
//! 2. A history file may be re-written ("vacuumed"). This involves reading in the file and writing
//!    a new one, while performing maintenance tasks: discarding items in an LRU fashion until we
//!    reach the desired maximum count, removing duplicates, and sorting them by timestamp
//!    (eventually, not implemented yet). The new file is atomically moved into place via `rename()`.
//!
//! 3. History files are mapped in via `mmap()`. This allows only storing one `usize` per item (its
//!    offset), and lazily loading items on demand, which reduces memory consumption.
//!
//! 4. Accesses to the history file need to be synchronized. This is achieved by functionality in
//!    `src/fs.rs`. By default, `flock()` is used for locking. If that is unavailable, an imperfect
//!    fallback solution attempts to detect races and retries if a race is detected.

use crate::{
    common::cstr2wcstring,
    env::{EnvSetMode, EnvVar},
    fs::{
        LOCKED_FILE_MODE, LockedFile, LockingMode, PotentialUpdate, WriteMethod, lock_and_load,
        rewrite_via_temporary_file,
    },
    threads::ThreadPool,
};
use fish_wcstringutil::{subsequence_in_string, trim};
use fish_widestring::subslice_position;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString},
    fs::File,
    io::{BufRead, BufWriter, Write},
    mem::MaybeUninit,
    ops::ControlFlow,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bitflags::bitflags;
use nix::{fcntl::OFlag, sys::stat::Mode};
use rand::Rng;

use crate::{
    ast::{self, Kind, Node},
    common::{CancelChecker, bytes2wcstring, valid_var_name},
    env::{EnvMode, EnvStack, Environment},
    expand::{ExpandFlags, expand_one, replace_home_directory_with_tilde},
    fds::wopen_cloexec,
    flog::{flog, flogf},
    fs::fsync,
    highlight::highlight_and_colorize,
    history::file::{map_file, time_to_seconds},
    history::jsonl_backend::HistoryFile,
    history::yaml_compat,
    io::IoStreams,
    localization::wgettext_fmt,
    operation_context::{EXPANSION_LIMIT_BACKGROUND, OperationContext},
    parse_constants::ParseTreeFlags,
    parse_util::{detect_parse_errors, unescape_wildcards},
    parser::Parser,
    path::{path_get_config, path_get_data, path_is_valid},
    prelude::*,
    threads::assert_is_background_thread,
    wildcard::{ANY_STRING, wildcard_match},
    wutil::{FileId, INVALID_FILE_ID, file_id_for_file, wrealpath, wstat, wunlink},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchType {
    /// Search for commands exactly matching the given string.
    Exact,
    /// Search for commands containing the given string.
    Contains,
    /// Search for commands starting with the given string.
    Prefix,
    /// Search for commands where any line matches the given string.
    LinePrefix,
    /// Search for commands containing the given glob pattern.
    ContainsGlob,
    /// Search for commands starting with the given glob pattern.
    PrefixGlob,
    /// Search for commands containing the given string as a subsequence
    ContainsSubsequence,
}

/// Ways that a history item may be written to disk (or omitted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PersistenceMode {
    /// The history item is written to disk normally
    #[default]
    Disk,
    /// The history item is stored in-memory only, not written to disk
    Memory,
    /// The history item is stored in-memory and deleted when a new item is added
    Ephemeral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// This is the history namespace we use by default if the user has not set env var fish_history.
const DFLT_FISH_HISTORY_NAMESPACE: &wstr = L!("fish");

pub const VACUUM_FREQUENCY: usize = 25;

struct TimeProfiler {
    what: &'static str,
    start: SystemTime,
}

impl TimeProfiler {
    fn new(what: &'static str) -> Self {
        let start = SystemTime::now();
        Self { what, start }
    }
}

impl Drop for TimeProfiler {
    fn drop(&mut self) {
        if let Ok(duration) = self.start.elapsed() {
            let ns_per_ms = 1_000_000;
            let ms = duration.as_millis();
            let ns = duration.as_nanos() - (ms * ns_per_ms);
            flogf!(
                profile_history,
                "%s: %d.%06d ms",
                self.what,
                ms as u64, // todo!("remove cast")
                ns as u32
            );
        } else {
            flogf!(profile_history, "%s: ??? ms", self.what);
        }
    }
}

pub type PathList = Vec<WString>;

/// History items are identified by a u64, where the high 48 bits are the number of milliseconds since the epoch and the low 16 bits are a nonce.
/// Multiple records that all contribute to an item will have the same ID.
/// Note this gives thousands of years at millisecond resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HistoryItemId(u64);

impl HistoryItemId {
    const NONCE_BITS: u32 = 16;

    /// Create a new history item identifier from a timestamp and nonce.
    pub fn new(timestamp: SystemTime, nonce: u16) -> Self {
        // Note we are unconcerned with wraparound here: should the clock be set to thousands of years in the future
        // the worst case is we get items with wrong timestamps.
        let millis = timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        Self((millis << Self::NONCE_BITS) | u64::from(nonce))
    }

    /// Extract the timestamp (millisecond precision) encoded in this identifier.
    pub fn timestamp(self) -> SystemTime {
        let millis = self.0 >> Self::NONCE_BITS;
        UNIX_EPOCH + Duration::from_millis(millis)
    }

    /// Return the raw 64-bit representation.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Construct directly from a raw 64-bit identifier.
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug)]
pub struct HistoryItem {
    /// The unique identifier for this item, which includes a timestamp.
    pub id: HistoryItemId,
    /// The command of the entry.
    pub contents: WString,
    /// Paths that we require to be valid for this item to be autosuggested.
    pub required_paths: Vec<WString>,
    /// The exit code of the command.
    pub exit_code: Option<i32>,
    /// Duration of command execution in milliseconds.
    pub duration: Option<u64>,
    /// Working directory where the command was executed.
    pub cwd: Option<WString>,
    /// Session identifier.
    pub session_id: Option<u64>,
    /// Whether to write this item to disk.
    /// This is itself not written to disk.
    pub persist_mode: PersistenceMode,
}

impl HistoryItem {
    /// Construct a history item with the given id, leaving other fields empty.
    ///
    /// This is the primary constructor for `HistoryItem`. Use it with struct update syntax
    /// to set specific fields:
    ///
    /// ```ignore
    /// // Full item with content
    /// let item = HistoryItem {
    ///     contents: text,
    ///     persist_mode: mode,
    ///     ..HistoryItem::with_id(id)
    /// };
    ///
    /// // Partial update with just required paths
    /// let update = HistoryItem {
    ///     required_paths: paths,
    ///     ..HistoryItem::with_id(id)
    /// };
    /// ```
    pub fn with_id(id: HistoryItemId) -> Self {
        Self {
            id,
            contents: WString::new(),
            required_paths: Vec::new(),
            exit_code: None,
            duration: None,
            cwd: None,
            session_id: None,
            persist_mode: PersistenceMode::Disk,
        }
    }

    /// Returns the text as a string.
    pub fn str(&self) -> &wstr {
        &self.contents
    }

    /// Returns whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Returns whether our contents matches a search term.
    pub fn matches_search(&self, term: &wstr, typ: SearchType, case_sensitive: bool) -> bool {
        // Note that 'term' has already been lowercased when constructing the
        // search object if we're doing a case insensitive search.
        let content_to_match = if case_sensitive {
            Cow::Borrowed(&self.contents)
        } else {
            Cow::Owned(self.contents.to_lowercase())
        };

        match typ {
            SearchType::Exact => term == *content_to_match,
            SearchType::Contains => {
                subslice_position(content_to_match.as_slice(), term.as_slice()).is_some()
            }
            SearchType::Prefix => content_to_match.as_slice().starts_with(term.as_slice()),
            SearchType::LinePrefix => content_to_match
                .as_char_slice()
                .split(|&c| c == '\n')
                .any(|line| line.starts_with(term.as_char_slice())),
            SearchType::ContainsGlob => {
                let mut pat = unescape_wildcards(term);
                if !pat.starts_with(ANY_STRING) {
                    pat.insert(0, ANY_STRING);
                }
                if !pat.ends_with(ANY_STRING) {
                    pat.push(ANY_STRING);
                }
                wildcard_match(content_to_match.as_ref(), &pat, false)
            }
            SearchType::PrefixGlob => {
                let mut pat = unescape_wildcards(term);
                if !pat.ends_with(ANY_STRING) {
                    pat.push(ANY_STRING);
                }
                wildcard_match(content_to_match.as_ref(), &pat, false)
            }
            SearchType::ContainsSubsequence => subsequence_in_string(term, &content_to_match),
        }
    }

    /// Returns the timestamp for creating this history item.
    pub fn timestamp(&self) -> SystemTime {
        self.id.timestamp()
    }

    /// Returns whether this item should be persisted (written to disk).
    pub fn should_write_to_disk(&self) -> bool {
        self.persist_mode == PersistenceMode::Disk
    }

    /// Get the list of arguments which referred to files.
    /// This is used for autosuggestion hinting.
    pub fn get_required_paths(&self) -> &[WString] {
        &self.required_paths
    }

    /// Set the list of arguments which referred to files.
    /// This is used for autosuggestion hinting.
    pub fn set_required_paths(&mut self, paths: Vec<WString>) {
        self.required_paths = paths;
    }

    /// Merge fields from another item. Only updates fields that are Some/non-empty.
    pub fn merge(&mut self, other: HistoryItem) {
        if !other.contents.is_empty() {
            self.contents = other.contents;
        }

        if other.exit_code.is_some() {
            self.exit_code = other.exit_code;
        }
        if !other.required_paths.is_empty() {
            self.required_paths = other.required_paths;
        }
        if other.duration.is_some() {
            self.duration = other.duration;
        }
        if other.cwd.is_some() {
            self.cwd = other.cwd;
        }
        if other.session_id.is_some() {
            self.session_id = other.session_id;
        }
    }
}

static HISTORIES: Mutex<BTreeMap<WString, Arc<History>>> = Mutex::new(BTreeMap::new());

/// When deleting, whether the deletion should be only for this session or for all sessions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeletionScope {
    SessionOnly,
    AllSessions,
}

struct HistoryImpl {
    /// The name of this list. Used for picking a suitable filename and for switching modes.
    name: WString,
    /// Optional custom directory for the history file. If None, uses path_get_data().
    /// Primarily for testing.
    custom_directory: Option<WString>,
    /// New items. Note that these are NOT discarded on save. We need to keep these around so we can
    /// distinguish between items in our history and items in the history of other shells that were
    /// started after we were started.
    new_items: Vec<HistoryItem>,
    /// Whether we have a pending item. If so, the most recently added item is ignored by
    /// item_at_index.
    has_pending_item: bool, // false
    /// Deleted item contents, and the scope of the deletion.
    deleted_items: HashMap<WString, DeletionScope>,
    /// The history file contents.
    file_contents: Option<HistoryFile>,
    /// The file ID of the history file.
    history_file_id: FileId, // INVALID_FILE_ID
    /// The boundary timestamp distinguishes old items from new items. Items whose timestamps are <=
    /// the boundary are considered "old". Items whose timestamps are > the boundary are new, and are
    /// ignored by this instance (unless they came from this instance). The timestamp may be adjusted
    /// by incorporate_external_changes().
    boundary_timestamp: SystemTime,
    /// Next nonce used when constructing [`HistoryItemId`]s.
    next_item_id_nonce: u16,
    /// How many items we add until the next vacuum. Initially a random value.
    countdown_to_vacuum: Option<usize>,
    /// Thread pool for background operations.
    thread_pool: Arc<ThreadPool>,
}

impl HistoryImpl {
    /// Returns the canonical path for the history file, or `Ok(None)` in private mode.
    /// An error is returned if obtaining the data directory fails.
    /// Because the `path_get_data` function does not return error information,
    /// we cannot provide more detail about the reason for the failure here.
    fn history_file_path(&self) -> std::io::Result<Option<WString>> {
        if self.name.is_empty() {
            return Ok(None);
        }

        let mut path = if let Some(custom_dir) = &self.custom_directory {
            custom_dir.clone()
        } else if let Some(data_path) = path_get_data() {
            data_path
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Error obtaining data directory. This is a manually constructed error which does not indicate why this happened.",
            ));
        };

        path.push('/');
        path.push_utfstr(&self.name);
        path.push_utfstr(L!("_history.jsonl"));

        // For custom directories, skip wrealpath since file may not exist yet
        if self.custom_directory.is_some() {
            Ok(Some(path))
        } else if let Some(canonicalized_path) = wrealpath(&path) {
            Ok(Some(canonicalized_path))
        } else {
            Err(std::io::Error::other(format!(
                "wrealpath failed to produce a canonical version of '{path}'."
            )))
        }
    }

    /// Add a new history item to the end. If `pending` is set, the item will not be returned by
    /// `item_at_index()` until a call to `resolve_pending()`. Pending items are tracked with an
    /// offset into the array of new items, so adding a non-pending item has the effect of resolving
    /// all pending items.
    fn add(&mut self, item: HistoryItem, pending: bool) -> HistoryItemId {
        // We use empty items as sentinels to indicate the end of history.
        // Do not allow them to be added (#6032).
        assert!(!item.contents.is_empty(), "Cannot add empty history item");

        let id = item.id;

        let should_write = item.should_write_to_disk();
        let json_str: Option<String> = if should_write {
            Some(item.to_json_line())
        } else {
            None
        };

        // Add to our in-memory list and maybe write to disk.
        self.new_items.push(item);
        self.has_pending_item = pending;
        if let Some(json_str) = json_str {
            self.append_to_disk(|file| file.write_all(json_str.as_bytes()));
            self.maybe_vacuum();
        }
        id
    }

    /// Check if vacuum is needed and trigger it.
    fn maybe_vacuum(&mut self) {
        // Initialize countdown to a random value if not set yet.
        let countdown = self
            .countdown_to_vacuum
            .get_or_insert_with(|| rand::rng().random_range(0..VACUUM_FREQUENCY));

        // Check if it's time to vacuum.
        let mut vacuum = false;
        if *countdown == 0 {
            *countdown = VACUUM_FREQUENCY;
            vacuum = true;
        }

        // Update countdown.
        assert!(*countdown > 0);
        *countdown -= 1;

        if vacuum {
            self.vacuum();
        }
    }

    /// Helper to append data to the history file.
    /// Takes a closure that writes to the file.
    fn append_to_disk<F>(&mut self, write_fn: F)
    where
        F: FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    {
        if self.name.is_empty() {
            return;
        }

        if let Ok(Some(history_path)) = self.history_file_path() {
            let result = (|| {
                let mut locked_file =
                    LockedFile::new(LockingMode::Exclusive(WriteMethod::Append), &history_path)?;

                write_fn(locked_file.get_mut())?;
                fsync(locked_file.get())?;

                self.history_file_id = file_id_for_file(locked_file.get());

                Ok::<(), std::io::Error>(())
            })();

            if let Err(e) = result {
                flog!(history, "Failed to append to disk:", e);
            }
        }
    }

    /// Internal function.
    fn clear_file_state(&mut self) {
        // Erase everything we know about our file.
        self.file_contents = None;
    }

    /// Returns the current timestamp for new items.
    fn timestamp_now(&self) -> SystemTime {
        SystemTime::now()
    }

    /// Generate a unique [`HistoryItemId`], incrementing our nonce each time.
    fn next_item_id(&mut self) -> HistoryItemId {
        let nonce = self.next_item_id_nonce;
        self.next_item_id_nonce = self.next_item_id_nonce.wrapping_add(1);
        HistoryItemId::new(self.timestamp_now(), nonce)
    }

    /// Create a new history item with a fresh ID.
    ///
    /// Use struct update syntax to set the fields you need:
    ///
    /// ```ignore
    /// let item = HistoryItem {
    ///     contents: text,
    ///     persist_mode: PersistenceMode::Disk,
    ///     ..imp.new_item()
    /// };
    /// imp.add(item, false);
    /// ```
    fn new_item(&mut self) -> HistoryItem {
        HistoryItem::with_id(self.next_item_id())
    }

    /// Loads old items if necessary.
    /// Return a reference to the loaded history file.
    fn load_old_if_needed(&mut self) -> &HistoryFile {
        if let Some(ref file_contents) = self.file_contents {
            return file_contents;
        }
        let Ok(Some(history_path)) = self.history_file_path() else {
            return self.file_contents.insert(HistoryFile::create_empty());
        };

        let _profiler = TimeProfiler::new("load_old");
        let file_contents = match lock_and_load(&history_path, map_file) {
            Ok((file_id, history_file)) => {
                self.history_file_id = file_id;
                let _profiler = TimeProfiler::new("populate_from_file_contents");
                let file_contents =
                    HistoryFile::from_data(history_file, Some(self.boundary_timestamp));
                flogf!(
                    history,
                    "Loaded %u old item fragments",
                    file_contents.line_count()
                );
                file_contents
            }
            Err(e) => {
                flog!(history_file, "Error reading from history file:", e);
                HistoryFile::create_empty()
            }
        };
        self.file_contents.insert(file_contents)
    }

    /// Removes trailing ephemeral items.
    /// Ephemeral items have leading spaces, and can only be retrieved immediately; adding any item
    /// removes them.
    fn remove_ephemeral_items(&mut self) {
        while matches!(
            self.new_items.last(),
            Some(&HistoryItem {
                persist_mode: PersistenceMode::Ephemeral,
                ..
            })
        ) {
            self.new_items.pop();
        }
    }

    /// Given an existing history file, write a new history file to `dst`.
    fn rewrite_to_temporary_file(
        &self,
        existing_file: &File,
        dst: &mut File,
    ) -> std::io::Result<usize> {
        // We are reading FROM existing_file and writing TO dst
        // When we rewrite the history, the number of items we keep.
        // Assume ~256 bytes per item; this yields a max size of 134 MB.
        const HISTORY_MAX_ITEMS: usize = 1024 * 512;

        // Default buffer size for flushing to the history file.
        const HISTORY_OUTPUT_BUFFER_SIZE: usize = 64 * 1024;

        // Read in existing items (which may have changed out from underneath us, so don't trust our
        // old file contents).
        let file_id = file_id_for_file(existing_file);
        let mmap = map_file(existing_file, file_id)?;
        let mut local_file = HistoryFile::from_data(mmap, None);
        local_file.shrink_to_max_records(HISTORY_MAX_ITEMS);
        let mut buffer = BufWriter::with_capacity(HISTORY_OUTPUT_BUFFER_SIZE, dst);
        let mut items_written = 0;
        for old_item in local_file.items() {
            if old_item.is_empty() {
                continue;
            }

            // Check if this item should be deleted.
            if let Some(&scope) = self.deleted_items.get(old_item.str()) {
                // If old item is newer than session always erase if in deleted.
                // If old item is older and in deleted items don't erase if added by clear_session.
                let delete = old_item.timestamp() > self.boundary_timestamp
                    || scope == DeletionScope::AllSessions;
                if delete {
                    continue;
                }
            }
            old_item.write_to(&mut buffer)?;
            items_written += 1;
        }
        buffer.flush()?;
        Ok(items_written)
    }

    /// Saves history by rewriting the file.
    fn rewrite(&mut self, history_path: &wstr) -> std::io::Result<()> {
        use std::time::Instant;

        flogf!(
            history,
            "Vacuuming history with %u in-memory items",
            self.new_items.len()
        );

        let start_time = Instant::now();

        let rewrite =
            |old_file: &File, tmp_file: &mut File| -> std::io::Result<PotentialUpdate<usize>> {
                let result = self.rewrite_to_temporary_file(old_file, tmp_file);
                match result {
                    Ok(count) => Ok(PotentialUpdate {
                        do_save: true,
                        data: count,
                    }),
                    Err(err) => {
                        flog!(
                            history_file,
                            "Error writing to temporary history file:",
                            err
                        );
                        Err(err)
                    }
                }
            };

        let (file_id, potential_update) = rewrite_via_temporary_file(history_path, rewrite)?;
        self.history_file_id = file_id;

        let elapsed = start_time.elapsed();
        flogf!(
            history,
            "Vacuumed %u items in %u.%03u seconds",
            potential_update.data,
            elapsed.as_secs(),
            elapsed.subsec_millis()
        );

        // We deleted our deleted items.
        self.deleted_items.clear();

        // Our history has been written to the file, so clear our state so we can re-reference the
        // file.
        self.clear_file_state();

        Ok(())
    }

    /// Performs a vacuum (full rewrite) of the history file.
    /// Items have already been written incrementally, so this consolidates the file.
    fn vacuum(&mut self) {
        if self.name.is_empty() {
            // Incognito mode - just clean up state.
            self.deleted_items.clear();
            self.clear_file_state();
            return;
        }
        let history_path = match self.history_file_path() {
            Ok(Some(path)) => path,
            _ => return,
        };

        if let Err(e) = self.rewrite(&history_path) {
            flog!(history, "Vacuum failed:", e);
        }
    }

    /// Saves history.
    /// As history is written immediately, this just performs a vacuum if necessary.
    fn save(&mut self, vacuum: bool) {
        if self.name.is_empty() {
            // We're in the "incognito" mode. Pretend we've saved the history.
            self.deleted_items.clear();
            self.clear_file_state();
            return;
        }

        // Rewrite the history file if requested or if we have deleted items.
        if vacuum || !self.deleted_items.is_empty() {
            self.vacuum();
        }
    }

    /// Create a new HistoryImpl.
    fn new(name: WString, custom_directory: Option<WString>) -> Self {
        let next_item_id_nonce = rand::rng().random_range(0..65536) as u16;
        Self {
            name,
            custom_directory,
            new_items: vec![],
            has_pending_item: false,
            deleted_items: HashMap::new(),
            file_contents: None,
            history_file_id: INVALID_FILE_ID,
            boundary_timestamp: SystemTime::now(),
            next_item_id_nonce,
            countdown_to_vacuum: None,
            // Up to 8 threads, no soft min.
            thread_pool: ThreadPool::new(0, 8),
        }
    }

    /// Returns whether this is using the default name.
    fn is_default(&self) -> bool {
        self.name == DFLT_FISH_HISTORY_NAMESPACE
    }

    /// Determines whether the history is empty. Unfortunately this cannot be const, since it may
    /// require populating the history.
    fn is_empty(&mut self) -> bool {
        // If we have new items, we're not empty.
        if !self.new_items.is_empty() {
            return false;
        }

        if let Some(file_contents) = &self.file_contents {
            // If we've loaded old items, see if we have any items.
            file_contents.is_empty()
        } else {
            // If we have not loaded old items, don't actually load them (which may be expensive); just
            // stat the file and see if it exists and is nonempty.
            let Ok(Some(where_)) = self.history_file_path() else {
                return true;
            };

            if let Ok(md) = wstat(&where_) {
                // We're empty if the file is empty.
                md.len() == 0
            } else {
                // Access failed, assume missing.
                true
            }
        }
    }

    /// Remove a history item.
    fn remove(&mut self, str_to_remove: &wstr) {
        // Add to our list of deleted items.
        self.deleted_items
            .insert(str_to_remove.to_owned(), DeletionScope::AllSessions);

        for idx in (0..self.new_items.len()).rev() {
            let matched = self.new_items[idx].str() == str_to_remove;
            if matched {
                self.new_items.remove(idx);
            }
        }
    }

    /// Resolves any pending history items, so that they may be returned in history searches.
    fn resolve_pending(&mut self) {
        self.has_pending_item = false;
    }

    /// Irreversibly clears history.
    fn clear(&mut self) {
        self.new_items.clear();
        self.deleted_items.clear();
        self.file_contents = None;
        if let Ok(Some(filename)) = self.history_file_path() {
            let _ = wunlink(&filename);
        }
        self.clear_file_state();
    }

    /// Clears only session.
    fn clear_session(&mut self) {
        for item in &self.new_items {
            self.deleted_items
                .insert(item.str().to_owned(), DeletionScope::SessionOnly);
        }

        self.new_items.clear();
    }

    // Return the path for the history file back when it was in the config path, if it exists.
    fn get_legacy_config_history_path(&self) -> Option<WString> {
        let mut old_file = path_get_config()?;

        old_file.push('/');
        old_file.push_utfstr(&self.name);
        old_file.push_str("_history");

        Some(old_file)
    }

    // Return the path for the history file in the yaml format
    // This is just the default path with no extension.
    fn get_legacy_yaml_history_path(&self) -> Option<WString> {
        let mut jsonl_path = self.history_file_path().ok()??;
        if !jsonl_path.ends_with(".jsonl") {
            return None;
        }
        jsonl_path.truncate(jsonl_path.len() - ".jsonl".len());
        Some(jsonl_path)
    }

    /// Populate from a yaml history file at the given path, migrating it to the jsonl format at the given new path.
    /// Returns true if successful.
    fn populate_from_legacy_yaml_path(&mut self, old_path: &WString, new_path: &WString) -> bool {
        let _profiler = TimeProfiler::new("migrate_legacy");
        let Ok(old_file) = wopen_cloexec(old_path, OFlag::O_RDONLY, Mode::empty()) else {
            return false;
        };
        let file_id = file_id_for_file(&old_file);
        let mmap = match map_file(&old_file, file_id) {
            Ok(mmap) => mmap,
            Err(err) => {
                flog!(history_file, "Error when reading legacy history file:", err);
                return false;
            }
        };

        // Clear must come after we've retrieved the new_file name, and before we open
        // destination file descriptor, since it destroys the name and the file.
        self.clear();

        let dst_file = match wopen_cloexec(
            new_path,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
            LOCKED_FILE_MODE,
        ) {
            Ok(file) => file,
            Err(err) => {
                flog!(history_file, "Error when writing history file:", err);
                return false;
            }
        };

        let mut count = 0;
        let result = || -> std::io::Result<()> {
            let mut buffer = BufWriter::new(dst_file);
            for item in yaml_compat::iterate_fish_2_0_history(mmap.as_ref()) {
                if item.is_empty() {
                    continue;
                }
                item.write_to(&mut buffer)?;
                count += 1;
            }
            buffer.flush()
        }();

        if let Err(err) = result {
            flog!(history_file, "Error when writing history file:", err);
            return false;
        }

        let duration_ms = _profiler
            .start
            .elapsed()
            .map_or(0, |d| d.as_millis() as u64);
        flogf!(
            history,
            "Migrated history from legacy file '%s' to new jsonl file '%s': %u items in %u ms",
            old_path,
            new_path,
            count,
            duration_ms
        );
        true
    }

    /// Populates from older locations.
    fn populate_from_legacy_paths(&mut self) {
        let Ok(Some(new_path)) = self.history_file_path() else {
            return;
        };
        let old_path_getters = [
            Self::get_legacy_yaml_history_path,
            Self::get_legacy_config_history_path,
        ];
        for get_old_path in old_path_getters {
            if let Some(old_path) = get_old_path(self) {
                if self.populate_from_legacy_yaml_path(&old_path, &new_path) {
                    return;
                }
            }
        }
    }

    /// Import a bash command history file. Bash's history format is very simple: just lines with
    /// `#`s for comments. Ignore a few commands that are bash-specific. It makes no attempt to
    /// handle multiline commands. We can't actually parse bash syntax and the bash history file
    /// does not unambiguously encode multiline commands.
    fn populate_from_bash<R: BufRead>(&mut self, contents: R) {
        // Create synthetic timestamps starting from 15 minutes ago.
        let base_time = SystemTime::now() - Duration::from_secs(15 * 60);
        let mut synthetic_timestamp = base_time;

        // Process the entire history file until EOF is observed.
        for line in contents.split(b'\n') {
            let Ok(line) = line else {
                break;
            };
            let wide_line = trim(bytes2wcstring(&line), None);
            // Add this line if it doesn't contain anything we know we can't handle.
            if should_import_bash_history_line(&wide_line) {
                let item = HistoryItem {
                    contents: wide_line,
                    persist_mode: PersistenceMode::Disk,
                    ..HistoryItem::with_id(HistoryItemId::new(synthetic_timestamp, 0))
                };
                self.add(item, /*pending=*/ false);
                synthetic_timestamp += Duration::from_millis(1);
            }
        }
    }

    /// Incorporates the history of other shells into this history.
    fn incorporate_external_changes(&mut self) {
        // To incorporate new items, we simply update our timestamp to now, so that items from previous
        // instances get added. We then clear the file state so that we remap the file. Note that this
        // is somewhat expensive because we will be going back over old items. An optimization would be
        // to preserve old_item_offsets so that they don't have to be recomputed. (However, then items
        // *deleted* in other instances would not show up here).
        let new_timestamp = SystemTime::now();

        // If for some reason the clock went backwards, we don't want to start dropping items; therefore
        // we only do work if time has progressed. This also makes multiple calls cheap.
        if new_timestamp > self.boundary_timestamp {
            self.boundary_timestamp = new_timestamp;
            self.clear_file_state();

            // We also need to erase new items, since we go through those first, and that means we
            // will not properly interleave them with items from other instances.
            // We'll pick them up from the file (#2312)
            // TODO: this will drop items that had no_persist set, how can we avoid that while still
            // properly interleaving?
            self.new_items.clear();
        }
    }

    /// Gets all the history into a list. This is intended for the $history environment variable.
    /// This may be long!
    fn get_history(&mut self) -> Vec<WString> {
        let mut result = vec![];
        // If we have a pending item, we skip the first encountered (i.e. last) new item.
        let mut next_is_pending = self.has_pending_item;
        let mut seen = HashSet::new();

        // Append new items.
        for item in self.new_items.iter().rev() {
            // Skip a pending item if we have one.
            if next_is_pending {
                next_is_pending = false;
                continue;
            }

            if seen.insert(item.str().to_owned()) {
                result.push(item.str().to_owned());
            }
        }

        // Append old items.
        let file_contents = self.load_old_if_needed();
        for item in file_contents.items().rev() {
            if item.is_empty() {
                continue;
            }
            if seen.insert(item.str().to_owned()) {
                result.push(item.str().to_owned());
            }
        }
        result
    }

    /// Let indexes be a list of one-based indexes into the history, matching the interpretation of
    /// `$history`. That is, `$history[1]` is the most recently executed command. Values less than one
    /// are skipped. Return a mapping from index to history item text.
    fn items_at_indexes(
        &mut self,
        indexes: impl IntoIterator<Item = usize>,
    ) -> HashMap<usize, WString> {
        let mut result = HashMap::new();
        for idx in indexes {
            // If this is the first time the index is encountered, we have to go fetch the item.
            #[allow(clippy::map_entry)] // looks worse
            if !result.contains_key(&idx) {
                // New key.
                let contents = match self.item_at_index(idx) {
                    None => WString::new(),
                    Some(Cow::Borrowed(HistoryItem { contents, .. })) => contents.clone(),
                    Some(Cow::Owned(HistoryItem { contents, .. })) => contents,
                };
                result.insert(idx, contents);
            }
        }
        result
    }

    /// Find a history item by its ID. Returns a mutable reference if found.
    fn find_item_by_id_mut(&mut self, id: HistoryItemId) -> Option<&mut HistoryItem> {
        // Search from end (most recent items first)
        self.new_items.iter_mut().rev().find(|item| item.id == id)
    }

    /// Emit a metadata update for a history item.
    /// Updates the in-memory item and writes the update to disk immediately.
    fn emit_update(&mut self, update: HistoryItem) {
        let id = update.id;

        let Some(item) = self.find_item_by_id_mut(id) else {
            return;
        };
        let should_write = item.should_write_to_disk();
        let json_str = if should_write {
            Some(update.to_json_line())
        } else {
            None
        };

        item.merge(update);
        if let Some(json_str) = json_str {
            self.append_to_disk(|file| file.write_all(json_str.as_bytes()));
        }
    }

    /// Return the specified history at the specified index. 0 is the index of the current
    /// commandline. (So the most recent item is at index 1.)
    /// Note that if an index is in bounds but the item could not be read, then an empty item is returned.
    /// None is returned only if the index is out of bounds.
    fn item_at_index(&mut self, mut idx: usize) -> Option<Cow<'_, HistoryItem>> {
        // 0 is considered an invalid index.
        if idx == 0 {
            return None;
        }
        idx -= 1;

        // Determine how many "resolved" (non-pending) items we have. We can have at most one pending
        // item, and it's always the last one.
        let mut resolved_new_item_count = self.new_items.len();
        if self.has_pending_item && resolved_new_item_count > 0 {
            resolved_new_item_count -= 1;
        }

        // idx == 0 corresponds to the last resolved item.
        if idx < resolved_new_item_count {
            return Some(Cow::Borrowed(
                &self.new_items[resolved_new_item_count - idx - 1],
            ));
        }

        // Now look in our old items.
        idx -= resolved_new_item_count;
        let file_contents = self.load_old_if_needed();
        // idx == 0 corresponds to last item.
        file_contents.get_from_back(idx).map(Cow::Owned)
    }

    /// Return the number of history entries.
    fn size(&mut self) -> usize {
        let mut new_item_count = self.new_items.len();
        if self.has_pending_item && new_item_count > 0 {
            new_item_count -= 1;
        }
        let old_item_count = self.load_old_if_needed().item_count();
        new_item_count + old_item_count
    }
}

fn string_could_be_path(potential_path: &wstr) -> bool {
    // Assume that things with leading dashes aren't paths.
    !(potential_path.is_empty() || potential_path.starts_with('-'))
}

/// Perform a search of `hist` for `search_string`. Invoke a function `func` for each match. If
/// `func` returns [`ControlFlow::Break`], stop the search.
fn do_1_history_search(
    hist: Arc<History>,
    search_type: SearchType,
    search_string: WString,
    case_sensitive: bool,
    mut func: impl FnMut(&HistoryItem) -> ControlFlow<(), ()>,
    cancel_check: &CancelChecker,
) {
    let mut searcher = HistorySearch::new_with(
        hist,
        search_string,
        search_type,
        if case_sensitive {
            SearchFlags::empty()
        } else {
            SearchFlags::IGNORE_CASE
        },
        0,
    );
    while !cancel_check() && searcher.go_to_next_match(SearchDirection::Backward) {
        if let ControlFlow::Break(()) = func(searcher.current_item()) {
            break;
        }
    }
}

/// Formats a single history record, including a trailing newline.
fn format_history_record(
    item: &HistoryItem,
    show_time_format: Option<&str>,
    null_terminate: bool,
    parser: &Parser,
    color_enabled: bool,
) -> WString {
    let mut result = WString::new();
    let seconds = time_to_seconds(item.timestamp());
    // This warns for musl, but the warning is useless to us - there is nothing we can or should do.
    #[allow(deprecated)]
    let seconds = seconds as libc::time_t;
    let mut timestamp = MaybeUninit::uninit();
    if let Some(show_time_format) = show_time_format.and_then(|s| CString::new(s).ok()) {
        if !unsafe { libc::localtime_r(&seconds, timestamp.as_mut_ptr()).is_null() } {
            const MAX_TIMESTAMP_LENGTH: usize = 100;
            let mut timestamp_str = [0_u8; MAX_TIMESTAMP_LENGTH];
            if unsafe {
                libc::strftime(
                    timestamp_str.as_mut_ptr().cast(),
                    MAX_TIMESTAMP_LENGTH,
                    show_time_format.as_ptr(),
                    timestamp.as_ptr(),
                )
            } != 0
            {
                // SAFETY: strftime terminates the string with a null byte. If there is insufficient
                // space, strftime returns 0.
                let timestamp_cstr = CStr::from_bytes_until_nul(&timestamp_str).unwrap();
                result.push_utfstr(&cstr2wcstring(timestamp_cstr));
            }
        }
    }

    let mut command = item.str().to_owned();
    if color_enabled {
        command = bytes2wcstring(&highlight_and_colorize(
            &command,
            &parser.context(),
            parser.vars(),
        ));
    }

    result.push_utfstr(&command);
    result.push(if null_terminate { '\0' } else { '\n' });
    result
}

/// Decide whether we ought to import a bash history line into fish. This is a very crude heuristic.
fn should_import_bash_history_line(line: &wstr) -> bool {
    if line.is_empty() {
        return false;
    }

    // The following are Very naive tests!

    // Skip comments.
    if line.starts_with('#') {
        return false;
    }

    // Skip lines with backticks because we don't have that syntax,
    // Skip brace expansions and globs because they don't work like ours
    // Skip lines that end with a backslash. We do not handle multiline commands from bash history.
    if line.chars().any(|c| matches!(c, '`' | '{' | '*' | '\\')) {
        return false;
    }

    // Skip lines with [[...]] and ((...)) since we don't handle those constructs.
    // "<<" here is a proxy for heredocs (and herestrings).
    for seq in [L!("[["), L!("]]"), L!("(("), L!("))"), L!("<<")] {
        if subslice_position(line.as_char_slice(), seq).is_some() {
            return false;
        }
    }

    if ast::parse(line, ParseTreeFlags::default(), None).errored() {
        return false;
    }

    // In doing this test do not allow incomplete strings. Hence the "false" argument.
    let mut errors = Vec::new();
    let _ = detect_parse_errors(line, Some(&mut errors), false);
    errors.is_empty()
}

pub struct History(Mutex<HistoryImpl>);

impl History {
    fn imp(&self) -> MutexGuard<'_, HistoryImpl> {
        self.0.lock().unwrap()
    }

    pub fn add_commandline(&self, s: WString) {
        let mut imp = self.imp();
        let item = HistoryItem {
            contents: s,
            persist_mode: PersistenceMode::Disk,
            ..imp.new_item()
        };
        imp.add(item, false);
    }

    /// Creates a new History with a custom directory path.
    /// The history file will be stored at `{directory}/{name}_history.jsonl`.
    pub fn new(name: &wstr, directory: Option<WString>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(HistoryImpl::new(
            name.to_owned(),
            directory,
        ))))
    }

    /// Returns the history with the given name, creating it if necessary, using the default data directory.
    /// This uses the HISTORIES global collection. Note it is possible to create a history without
    /// placing it into this collection.
    pub fn with_name(name: &wstr) -> Arc<Self> {
        let mut histories = HISTORIES.lock().unwrap();

        if let Some(hist) = histories.get(name) {
            Arc::clone(hist)
        } else {
            let hist = Self::new(name, None);
            histories.insert(name.to_owned(), Arc::clone(&hist));
            hist
        }
    }

    /// Returns whether this is using the default name.
    pub fn is_default(&self) -> bool {
        self.imp().is_default()
    }

    /// Determines whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.imp().is_empty()
    }

    /// Remove a history item.
    pub fn remove(&self, s: &wstr) {
        self.imp().remove(s);
    }

    /// Remove any trailing ephemeral items.
    pub fn remove_ephemeral_items(&self) {
        self.imp().remove_ephemeral_items();
    }

    /// Add a new pending history item to the end, and then begin file detection on the items to
    /// determine which arguments are paths. Arguments may be expanded (e.g. with PWD and variables)
    /// using the given `vars`. The item has the given `persist_mode`.
    pub fn add_pending_with_file_detection(
        self: &Arc<Self>,
        s: &wstr,
        vars: &EnvStack,
        persist_mode: PersistenceMode, /*=disk*/
    ) -> HistoryItemId {
        // We use empty items as sentinels to indicate the end of history.
        // Do not allow them to be added (#6032).
        assert!(!s.is_empty(), "Cannot add empty history item");

        // Find all arguments that look like they could be file paths.
        let ast = ast::parse(s, ParseTreeFlags::default(), None);

        let mut potential_paths = Vec::new();
        for node in ast.walk() {
            if let Kind::Argument(arg) = node.kind() {
                let potential_path = arg.source(s);
                if string_could_be_path(potential_path) {
                    potential_paths.push(potential_path.to_owned());
                }
            }
        }

        // If we got a path, we'll perform file detection for autosuggestion hinting.
        let wants_file_detection = !potential_paths.is_empty();
        let mut imp = self.imp();

        // Make our history item.
        let mut cwd = replace_home_directory_with_tilde(vars.get_pwd_slash(), vars);
        // Strip trailing slash unless it's the root directory.
        if cwd.len() > 1 && cwd.ends_with('/') {
            cwd.pop();
        }

        let item = HistoryItem {
            contents: s.to_owned(),
            persist_mode,
            cwd: Some(cwd),
            ..imp.new_item()
        };
        let item_id = imp.add(item, /*pending=*/ true);

        if wants_file_detection {
            // Check for which paths are valid on a background thread.
            // Don't hold the lock while we perform this file detection.
            let thread_pool = Arc::clone(&imp.thread_pool);
            drop(imp);
            let vars_snapshot = vars.snapshot();
            let self_clone = Arc::clone(self);
            thread_pool.perform(move || {
                // Don't hold the lock while we perform this file detection.
                let valid_file_paths = expand_and_detect_paths(potential_paths, &vars_snapshot);
                if !valid_file_paths.is_empty() {
                    // Create a partial item with just the valid paths
                    let update = HistoryItem {
                        required_paths: valid_file_paths,
                        ..HistoryItem::with_id(item_id)
                    };
                    self_clone.emit_update(update);
                }
            });
        }
        item_id
    }

    /// Emit a metadata update for a history item.
    /// Updates the in-memory item and writes the update to disk immediately.
    ///
    /// # Example
    /// ```
    /// use fish::history::{History, HistoryItem, HistoryItemId};
    /// use fish::prelude::*;
    ///
    /// // Create a history instance
    /// let history = History::new(L!("test_emit_update"), Some(0));
    /// let item_id = HistoryItemId::new(std::time::SystemTime::now(), 0);
    ///
    /// // Create an update item (shown for illustration; actual field access is module-private)
    /// let update = HistoryItem::with_id(item_id);
    ///
    /// // emit_update(update) would be called internally to write metadata
    /// ```
    pub fn emit_update(&self, update: HistoryItem) {
        self.imp().emit_update(update);
    }

    /// Resolves any pending history items, so that they may be returned in history searches.
    pub fn resolve_pending(&self) {
        self.imp().resolve_pending();
    }

    /// Saves history.
    /// As history is written immediately, this just performs a vacuum if necessary.
    pub fn save(&self) {
        self.imp().save(false);
    }

    /// Searches history.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        self: &Arc<Self>,
        parser: &Parser,
        streams: &mut IoStreams,
        search_type: SearchType,
        search_args: &[&wstr],
        show_time_format: Option<&str>,
        max_items: usize,
        case_sensitive: bool,
        null_terminate: bool,
        reverse: bool,
        cancel_check: &CancelChecker,
        color_enabled: bool,
    ) -> bool {
        let mut remaining = max_items;
        let mut collected = Vec::new();
        let mut output_error = false;

        // The function we use to act on each item.
        let mut func = |item: &HistoryItem| {
            if remaining == 0 {
                return ControlFlow::Break(());
            }
            remaining -= 1;
            let formatted_record = format_history_record(
                item,
                show_time_format,
                null_terminate,
                parser,
                color_enabled,
            );

            if reverse {
                // We need to collect this for later.
                collected.push(formatted_record.clone());
            } else {
                // We can output this immediately.
                if !streams.out.append(&formatted_record) {
                    // This can happen if the user hit Ctrl-C to abort (maybe after the first page?).
                    output_error = true;
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        };

        if search_args.is_empty() {
            // The user had no search terms; just append everything.
            do_1_history_search(
                Arc::clone(self),
                SearchType::Contains,
                WString::new(),
                true,
                &mut func,
                cancel_check,
            );
        } else {
            #[allow(clippy::unnecessary_to_owned)]
            for search_string in search_args.iter().copied() {
                if search_string.is_empty() {
                    streams
                        .err
                        .append(L!("Searching for the empty string isn't allowed"));
                    return false;
                }
                do_1_history_search(
                    Arc::clone(self),
                    search_type,
                    search_string.to_owned(),
                    case_sensitive,
                    &mut func,
                    cancel_check,
                );
            }
        }

        // Output any items we collected (which only happens in reverse).
        for item in collected.into_iter().rev() {
            if output_error {
                break;
            }

            if !streams.out.append(&item) {
                // Don't force an error if output was aborted (typically via Ctrl-C/SIGINT); just don't
                // try writing any more.
                output_error = true;
            }
        }

        // We are intentionally not returning false in case of an output error, as the user aborting the
        // output early (the most common case) isn't a reason to exit w/ a non-zero status code.
        true
    }

    /// Irreversibly clears history.
    pub fn clear(&self) {
        self.imp().clear();
    }

    /// Irreversibly clears history for the current session.
    pub fn clear_session(&self) {
        self.imp().clear_session();
    }

    /// Populates from older locations, migrating history.
    pub fn populate_from_legacy_paths(&self) {
        self.imp().populate_from_legacy_paths();
    }

    /// Populates from a bash history file.
    pub fn populate_from_bash<R: BufRead>(&self, contents: R) {
        self.imp().populate_from_bash(contents);
    }

    /// Incorporates the history of other shells into this history.
    pub fn incorporate_external_changes(&self) {
        self.imp().incorporate_external_changes();
    }

    /// Gets all the history into a list. This is intended for the $history environment variable.
    /// This may be long!
    pub fn get_history(&self) -> Vec<WString> {
        self.imp().get_history()
    }

    /// Let indexes be a list of one-based indexes into the history, matching the interpretation of
    /// `$history`. That is, `$history[1]` is the most recently executed command.
    /// Returns a mapping from index to history item text.
    pub fn items_at_indexes(
        &self,
        indexes: impl IntoIterator<Item = usize>,
    ) -> HashMap<usize, WString> {
        self.imp().items_at_indexes(indexes)
    }

    /// Return the specified history at the specified index. 0 is the index of the current
    /// commandline. (So the most recent item is at index 1.)
    pub fn item_at_index(&self, idx: usize) -> Option<HistoryItem> {
        self.imp().item_at_index(idx).map(Cow::into_owned)
    }

    /// Return the number of history entries.
    pub fn size(&self) -> usize {
        self.imp().size()
    }
}

bitflags! {
    /// Flags for history searching.
    #[derive(Clone, Copy, Default)]
    pub struct SearchFlags: u32 {
        /// If set, ignore case.
        const IGNORE_CASE = 1 << 0;
        /// If set, do not deduplicate, which can help performance.
        const NO_DEDUP = 1 << 1;
    }
}

/// Support for searching a history backwards.
/// Note this does NOT de-duplicate; it is the caller's responsibility to do so.
pub struct HistorySearch {
    /// The history in which we are searching.
    history: Arc<History>,
    /// The original search term.
    orig_term: WString,
    /// The (possibly lowercased) search term.
    canon_term: WString,
    /// Our search type.
    search_type: SearchType, // history_search_type_t::contains
    /// Our flags.
    flags: SearchFlags, // 0
    /// The current history item.
    current_item: Option<HistoryItem>,
    /// Index of the current history item.
    current_index: usize, // 0
    /// If deduping, the items we've seen.
    deduper: HashSet<WString>,
}

impl HistorySearch {
    #[cfg(test)]
    fn new(hist: Arc<History>, s: WString) -> Self {
        Self::new_with(hist, s, SearchType::Contains, SearchFlags::default(), 0)
    }
    #[cfg(test)]
    fn new_with_type(hist: Arc<History>, s: WString, search_type: SearchType) -> Self {
        Self::new_with(hist, s, search_type, SearchFlags::default(), 0)
    }
    #[cfg(test)]
    fn new_with_flags(hist: Arc<History>, s: WString, flags: SearchFlags) -> Self {
        Self::new_with(hist, s, SearchType::Contains, flags, 0)
    }
    /// Constructs a new history search.
    pub fn new_with(
        hist: Arc<History>,
        s: WString,
        search_type: SearchType,
        flags: SearchFlags,
        starting_index: usize,
    ) -> Self {
        let mut search = Self {
            history: hist,
            orig_term: s.clone(),
            canon_term: s,
            search_type,
            flags,
            current_item: None,
            current_index: starting_index,
            deduper: HashSet::new(),
        };

        if search.ignores_case() {
            search.canon_term = search.canon_term.to_lowercase();
        }

        search
    }

    /// Returns the original search term.
    pub fn original_term(&self) -> &wstr {
        &self.orig_term
    }

    pub fn prepare_to_search_after_deletion(&mut self) {
        assert_ne!(self.current_index, 0);
        self.current_index -= 1;
        self.current_item = None;
    }

    /// Finds the next search result. Returns `true` if one was found.
    pub fn go_to_next_match(&mut self, direction: SearchDirection) -> bool {
        let invalid_index = match direction {
            SearchDirection::Backward => usize::MAX,
            SearchDirection::Forward => 0,
        };

        if self.current_index == invalid_index {
            return false;
        }

        let mut index = self.current_index;
        loop {
            // Backwards means increasing our index.
            match direction {
                SearchDirection::Backward => index += 1,
                SearchDirection::Forward => index -= 1,
            }

            if self.current_index == invalid_index {
                return false;
            }

            // We're done if it's empty or we cancelled.
            let Some(item) = self.history.item_at_index(index) else {
                self.current_index = match direction {
                    SearchDirection::Backward => self.history.size() + 1,
                    SearchDirection::Forward => 0,
                };
                self.current_item = None;
                return false;
            };

            // Look for an item that matches and (if deduping) that we haven't seen before.
            if !item.matches_search(&self.canon_term, self.search_type, !self.ignores_case()) {
                continue;
            }

            // Skip if deduplicating.
            if self.dedup() && !self.deduper.insert(item.str().to_owned()) {
                continue;
            }

            // This is our new item.
            self.current_item = Some(item);
            self.current_index = index;
            return true;
        }
    }

    /// Move current index so there is `value` matches in between new and old indexes
    pub fn search_forward(&mut self, value: usize) {
        while self.go_to_next_match(SearchDirection::Forward) && self.deduper.len() <= value {}
        self.deduper.clear();
    }

    /// Returns the current search result item.
    ///
    /// # Panics
    ///
    /// This function panics if there is no current item.
    pub fn current_item(&self) -> &HistoryItem {
        self.current_item.as_ref().expect("No current item")
    }

    pub fn canon_term(&self) -> &wstr {
        &self.canon_term
    }

    /// Returns the current search result item contents.
    ///
    /// # Panics
    ///
    /// This function panics if there is no current item.
    pub fn current_string(&self) -> &wstr {
        self.current_item().str()
    }

    /// Returns the index of the current history item.
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Returns whether we are case insensitive.
    pub fn ignores_case(&self) -> bool {
        self.flags.contains(SearchFlags::IGNORE_CASE)
    }

    /// Returns whether we deduplicate items.
    fn dedup(&self) -> bool {
        !self.flags.contains(SearchFlags::NO_DEDUP)
    }
}

/// Saves the new history to disk.
pub fn save_all() {
    for hist in HISTORIES.lock().unwrap().values() {
        hist.save();
    }
}

/// Return the namespace (file name prefix) for the history file.
/// This is determined by the `fish_history` environment variable.
pub fn history_namespace(vars: &dyn Environment) -> WString {
    history_namespace_from_var(vars.get(L!("fish_history")))
}

pub fn history_namespace_from_var(history_name_var: Option<EnvVar>) -> WString {
    let Some(var) = history_name_var else {
        return DFLT_FISH_HISTORY_NAMESPACE.to_owned();
    };
    let namespace = var.as_string();
    if namespace.is_empty() || valid_var_name(&namespace) {
        namespace
    } else {
        flog!(
            error,
            wgettext_fmt!(
                "History namespace '%s' is not a valid variable name. Falling back to `%s`.",
                &namespace,
                DFLT_FISH_HISTORY_NAMESPACE
            ),
        );
        DFLT_FISH_HISTORY_NAMESPACE.to_owned()
    }
}

/// Given a list of proposed paths and a context, perform variable and home directory expansion,
/// and detect if the result expands to a value which is also the path to a file.
/// Wildcard expansions are suppressed - see implementation comments for why.
///
/// This is used for autosuggestion hinting. If we add an item to history, and one of its arguments
/// refers to a file, then we only want to suggest it if there is a valid file there.
/// This does disk I/O and may only be called in a background thread.
pub fn expand_and_detect_paths<P: IntoIterator<Item = WString>>(
    paths: P,
    vars: &dyn Environment,
) -> Vec<WString> {
    assert_is_background_thread();
    let working_directory = vars.get_pwd_slash();
    let ctx = OperationContext::background(vars, EXPANSION_LIMIT_BACKGROUND);
    let mut result = Vec::new();
    for path in paths {
        // Suppress cmdsubs since we are on a background thread and don't want to execute fish
        // script.
        // Suppress wildcards because we want to suggest e.g. `rm *` even if the directory
        // is empty (and so rm will fail); this is nevertheless a useful command because it
        // confirms the directory is empty.
        let mut expanded_path = path.clone();
        if expand_one(
            &mut expanded_path,
            ExpandFlags::FAIL_ON_CMDSUBST | ExpandFlags::SKIP_WILDCARDS,
            &ctx,
            None,
        ) && path_is_valid(&expanded_path, &working_directory)
        {
            // Note we return the original (unexpanded) path.
            result.push(path);
        }
    }

    result
}

/// Given a list of proposed paths and a context, expand each one and see if it refers to a file.
/// Wildcard expansions are suppressed.
/// Returns `true` if `paths` is empty or every path is valid.
pub fn all_paths_are_valid(paths: &[WString], ctx: &OperationContext<'_>) -> bool {
    assert_is_background_thread();
    let working_directory = ctx.vars().get_pwd_slash();
    let mut path = WString::new();
    for unexpanded_path in paths {
        path.clone_from(unexpanded_path);
        if ctx.check_cancel() {
            return false;
        }
        if !expand_one(
            &mut path,
            ExpandFlags::FAIL_ON_CMDSUBST | ExpandFlags::SKIP_WILDCARDS,
            ctx,
            None,
        ) {
            return false;
        }
        if !path_is_valid(&path, &working_directory) {
            return false;
        }
    }
    true
}

/// Sets private mode on. Once in private mode, it cannot be turned off.
pub fn start_private_mode(vars: &EnvStack) {
    let global_mode = EnvSetMode::new_at_early_startup(EnvMode::GLOBAL);
    vars.set_one(L!("fish_history"), global_mode, L!("").to_owned());
    vars.set_one(L!("fish_private_mode"), global_mode, L!("1").to_owned());
}

/// Queries private mode status.
pub fn in_private_mode(vars: &dyn Environment) -> bool {
    vars.get_unless_empty(L!("fish_private_mode")).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        History, HistoryItem, HistoryItemId, HistorySearch, PathList, PersistenceMode,
        SearchDirection, SearchFlags, SearchType, VACUUM_FREQUENCY, yaml_compat,
    };
    use crate::common::{ESCAPE_TEST_CHAR, osstr2wcstring};
    use crate::env::{EnvMode, EnvSetMode, EnvStack};
    use crate::fs::{LockedFile, WriteMethod};
    use crate::prelude::*;
    use fish_build_helper::workspace_root;
    use fish_wcstringutil::{string_prefixes_string, string_prefixes_string_case_insensitive};
    use rand::Rng;
    use rand::rngs::ThreadRng;
    use std::collections::{HashSet, VecDeque};
    use std::io::BufReader;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn history_contains(history: &History, txt: &wstr) -> bool {
        for i in 1.. {
            let Some(item) = history.item_at_index(i) else {
                break;
            };

            if item.str() == txt {
                return true;
            }
        }

        false
    }

    // Helper to create a history with a custom directory, for testing.
    fn create_test_history(name: &wstr, custom_dir: &wstr) -> Arc<History> {
        History::new(name, Some(custom_dir.to_owned()))
    }

    fn random_string(rng: &mut ThreadRng) -> WString {
        let mut result = WString::new();
        let max = rng.random_range(1..=32);
        for _ in 0..max {
            let c =
                char::from_u32(u32::try_from(1 + rng.random_range(0..ESCAPE_TEST_CHAR)).unwrap())
                    .unwrap();
            result.push(c);
        }
        result
    }

    #[test]
    fn test_history_allocates_monotonic_ids() {
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = Some(osstr2wcstring(tmpdir.path()));
        let history = History::new(L!("id_gen_history"), hist_dir);
        let mut last_item_id = HistoryItemId::new(UNIX_EPOCH, 0);
        assert!(last_item_id.0 == 0);
        for i in 0..100 {
            let item_id = {
                let mut imp = history.imp();
                let item = HistoryItem {
                    contents: format!("test {}", i).into(),
                    persist_mode: PersistenceMode::Disk,
                    ..imp.new_item()
                };
                imp.add(item, false)
            };
            assert!(item_id > last_item_id);
            last_item_id = item_id;
        }
    }

    #[test]
    fn test_history() {
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        macro_rules! test_history_matches {
            ($search:expr, $expected:expr) => {
                let expected: Vec<&wstr> = $expected;
                let mut found = vec![];
                while $search.go_to_next_match(SearchDirection::Backward) {
                    found.push($search.current_string().to_owned());
                }
                assert_eq!(expected, found);
            };
        }

        let items = [
            L!("Gamma"),
            L!("beta"),
            L!("BetA"),
            L!("Beta"),
            L!("alpha"),
            L!("AlphA"),
            L!("Alpha"),
            L!("alph"),
            L!("ALPH"),
            L!("ZZZ"),
        ];
        let nocase = SearchFlags::IGNORE_CASE;

        // Populate a history.
        let history = create_test_history(L!("test_history"), &hist_dir);
        history.clear();
        for s in items {
            history.add_commandline(s.to_owned());
        }

        // Helper to set expected items to those matching a predicate, in reverse order.
        let set_expected = |filt: fn(&wstr) -> bool| {
            let mut expected = vec![];
            for s in items {
                if filt(s) {
                    expected.push(s);
                }
            }
            expected.reverse();
            expected
        };

        // Items matching "a", case-sensitive.
        let mut searcher = HistorySearch::new(history.clone(), L!("a").to_owned());
        let expected = set_expected(|s| s.contains('a'));
        test_history_matches!(searcher, expected);

        // Items matching "alpha", case-insensitive.
        let mut searcher =
            HistorySearch::new_with_flags(history.clone(), L!("AlPhA").to_owned(), nocase);
        let expected = set_expected(|s| s.to_lowercase().find(L!("alpha")).is_some());
        test_history_matches!(searcher, expected);

        // Items matching "et", case-sensitive.
        let mut searcher = HistorySearch::new(history.clone(), L!("et").to_owned());
        let expected = set_expected(|s| s.find(L!("et")).is_some());
        test_history_matches!(searcher, expected);

        // Items starting with "be", case-sensitive.
        let mut searcher =
            HistorySearch::new_with_type(history.clone(), L!("be").to_owned(), SearchType::Prefix);
        let expected = set_expected(|s| string_prefixes_string(L!("be"), s));
        test_history_matches!(searcher, expected);

        // Items starting with "be", case-insensitive.
        let mut searcher = HistorySearch::new_with(
            history.clone(),
            L!("be").to_owned(),
            SearchType::Prefix,
            nocase,
            0,
        );
        let expected = set_expected(|s| string_prefixes_string_case_insensitive(L!("be"), s));
        test_history_matches!(searcher, expected);

        // Items exactly matching "alph", case-sensitive.
        let mut searcher =
            HistorySearch::new_with_type(history.clone(), L!("alph").to_owned(), SearchType::Exact);
        let expected = set_expected(|s| s == "alph");
        test_history_matches!(searcher, expected);

        // Items exactly matching "alph", case-insensitive.
        let mut searcher = HistorySearch::new_with(
            history.clone(),
            L!("alph").to_owned(),
            SearchType::Exact,
            nocase,
            0,
        );
        let expected = set_expected(|s| s.to_lowercase() == "alph");
        test_history_matches!(searcher, expected);

        // Test item removal case-sensitive.
        let mut searcher = HistorySearch::new(history.clone(), L!("Alpha").to_owned());
        test_history_matches!(searcher, vec![L!("Alpha")]);
        history.remove(L!("Alpha"));
        let mut searcher = HistorySearch::new(history.clone(), L!("Alpha").to_owned());
        test_history_matches!(searcher, vec![]);

        // Test history escaping and unescaping, yaml, etc.
        let mut before: VecDeque<HistoryItem> = VecDeque::new();
        let mut after: VecDeque<HistoryItem> = VecDeque::new();
        history.clear();
        let max = 100;
        let mut rng = rand::rng();
        for i in 1..=max {
            // Generate a value.
            let mut value = L!("test item ").to_owned() + &i.to_wstring()[..];

            // Maybe add some backslashes.
            if i % 3 == 0 {
                value += L!("(slashies \\\\\\ slashies)");
            }

            // Generate some paths.
            let paths: PathList = (0..rng.random_range(0..6))
                .map(|_| random_string(&mut rng))
                .collect();

            // Add this item - add returns the ID.
            let id = {
                let mut imp = history.imp();
                let item = HistoryItem {
                    contents: value.clone(),
                    persist_mode: PersistenceMode::Disk,
                    ..imp.new_item()
                };
                imp.add(item, false)
            };

            // Set paths via update.
            if !paths.is_empty() {
                let update = HistoryItem {
                    required_paths: paths.clone(),
                    ..HistoryItem::with_id(id)
                };
                history.emit_update(update);
            }

            // Create expected item for verification.
            let mut expected_item = HistoryItem {
                contents: value,
                ..HistoryItem::with_id(id)
            };
            expected_item.set_required_paths(paths);
            before.push_back(expected_item);
        }
        history.save();

        // Read items back in reverse order and ensure they're the same.
        for i in (1..=100).rev() {
            after.push_back(history.item_at_index(i).unwrap());
        }
        assert_eq!(before.len(), after.len());
        for i in 0..before.len() {
            let bef = &before[i];
            let aft = &after[i];
            assert_eq!(bef.str(), aft.str());
            assert_eq!(bef.timestamp(), aft.timestamp());
            assert_eq!(bef.get_required_paths(), aft.get_required_paths());
        }

        // Items should be explicitly added to the history.
        history.add_commandline(L!("test-command").into());
        assert!(history_contains(&history, L!("test-command")));

        // Clean up after our tests.
        history.clear();
    }

    // Wait until the next second.
    fn time_barrier() {
        let start = SystemTime::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                != start.duration_since(UNIX_EPOCH).unwrap().as_secs()
            {
                break;
            }
        }
    }

    fn generate_history_lines(item_count: usize, idx: usize) -> Vec<WString> {
        let mut result = Vec::with_capacity(item_count);
        for i in 0..item_count {
            result.push(sprintf!("%u %u", idx, i));
        }
        result
    }

    fn write_history_entries(dir: &wstr, item_count: usize, idx: usize) -> Arc<History> {
        // Called in child thread to modify history.
        // Partition the nonce space: use high 8 bits for thread ID, low 8 bits for counter.
        // This ensures each thread gets a non-overlapping range of nonces.
        let initial_nonce = (idx as u16) << 8;
        let hist = create_test_history(L!("race_test"), dir);
        hist.imp().next_item_id_nonce = initial_nonce;
        let hist_lines = generate_history_lines(item_count, idx);
        for line in hist_lines {
            hist.add_commandline(line);
            hist.save();
        }
        hist
    }

    #[test]
    fn test_history_races() {
        // Place history in a temp directory.
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        // Skip tests if we can't get an exclusive lock on a file in that directory.
        let tmp_balloon = tmpdir.path().join("history-races-test-balloon");
        std::fs::write(&tmp_balloon, []).unwrap();
        let mode = crate::fs::LockingMode::Exclusive(WriteMethod::RenameIntoPlace);
        if LockedFile::new(mode, &osstr2wcstring(&tmp_balloon)).is_err() {
            return;
        }

        // Testing history race conditions

        // Test concurrent history writing.
        // How many concurrent writers we have
        const RACE_COUNT: usize = 4;

        // How many items each writer makes
        const ITEM_COUNT: usize = 256;

        // Ensure history is clear.
        create_test_history(L!("race_test"), &hist_dir).clear();

        let mut children = Vec::with_capacity(RACE_COUNT);
        for i in 0..RACE_COUNT {
            let hist_dir = hist_dir.clone();
            children.push(std::thread::spawn(move || {
                write_history_entries(&hist_dir, ITEM_COUNT, i);
            }));
        }

        // Wait for all children.
        for child in children {
            child.join().unwrap();
        }

        // Compute the expected lines.
        let expected_lines: [Vec<WString>; RACE_COUNT] =
            std::array::from_fn(|i| generate_history_lines(ITEM_COUNT, i));

        // Ensure we consider the lines that have been outputted as part of our history.
        time_barrier();

        // Ensure that we got sane, sorted results.
        let hist = create_test_history(L!("race_test"), &hist_dir);

        // Get all history items (newest to oldest, deduplicated).
        let history_items = hist.get_history();

        // Create a set of all expected items (4 threads × 256 items each).
        let mut all_expected: HashSet<WString> = expected_lines
            .iter()
            .flat_map(|vec| vec.iter().cloned())
            .collect();

        // Verify count matches.
        assert_eq!(
            history_items.len(),
            RACE_COUNT * ITEM_COUNT,
            "Expected {} items, got {}",
            RACE_COUNT * ITEM_COUNT,
            history_items.len()
        );

        // Verify all items are expected.
        for item in &history_items {
            assert!(
                all_expected.remove(item),
                "Found unexpected item in history: {}",
                item
            );
        }

        // Verify all expected items were found.
        assert!(
            all_expected.is_empty(),
            "Some items not found in history. Missing: {:?}",
            all_expected
        );
        hist.clear();
    }

    #[test]
    fn test_history_external_rewrites() {
        // Place history in a temp directory.
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        // Write some history to disk.
        {
            let hist = write_history_entries(&hist_dir, VACUUM_FREQUENCY / 2, 0);
            hist.add_commandline("needle".into());
            hist.save();
        }
        std::thread::sleep(Duration::from_secs(1));

        // Read history from disk.
        let hist = create_test_history(L!("race_test"), &hist_dir);
        assert_eq!(hist.item_at_index(1).unwrap().str(), "needle");

        // Add items until we rewrite the file.
        // In practice this might be done by another shell.
        write_history_entries(&hist_dir, VACUUM_FREQUENCY, 0);

        for i in 1.. {
            if hist.item_at_index(i).unwrap().str() == "needle" {
                break;
            }
        }
    }

    #[test]
    fn test_history_merge() {
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        // In a single fish process, only one history is allowed to exist with the given name. But it's
        // common to have multiple history instances with the same name active in different processes,
        // e.g. when you have multiple shells open. We try to get that right and merge all their history
        // together. Test that case.
        const COUNT: usize = 3;
        let name = L!("merge_test");
        let hists = [
            create_test_history(name, &hist_dir),
            create_test_history(name, &hist_dir),
            create_test_history(name, &hist_dir),
        ];
        let texts = [L!("History 1"), L!("History 2"), L!("History 3")];
        let alt_texts = [
            L!("History Alt 1"),
            L!("History Alt 2"),
            L!("History Alt 3"),
        ];

        // Make sure history is clear.
        for hist in &hists {
            hist.clear();
        }

        // Make sure we don't add an item in the same second as we created the history.
        time_barrier();

        // Add a different item to each.
        for i in 0..COUNT {
            hists[i].add_commandline(texts[i].to_owned());
        }

        // Save them.
        for hist in &hists {
            hist.save();
        }

        // Make sure each history contains what it ought to, but they have not leaked into each other.
        #[allow(clippy::needless_range_loop)]
        for i in 0..COUNT {
            for j in 0..COUNT {
                let does_contain = history_contains(&hists[i], texts[j]);
                let should_contain = i == j;
                assert_eq!(should_contain, does_contain);
            }
        }

        // Make a new history. It should contain everything. The time_barrier() is so that the timestamp
        // is newer, since we only pick up items whose timestamp is before the birth stamp.
        time_barrier();
        let everything = create_test_history(name, &hist_dir);
        for text in texts {
            assert!(history_contains(&everything, text));
        }

        // Tell all histories to merge. Now everybody should have everything.
        for hist in &hists {
            hist.incorporate_external_changes();
        }

        // Everyone should also have items in the same order (#2312)
        let hist_vals1 = hists[0].get_history();
        for hist in &hists {
            assert_eq!(hist_vals1, hist.get_history());
        }

        // Add some more per-history items.
        for i in 0..COUNT {
            hists[i].add_commandline(alt_texts[i].to_owned());
        }
        // Everybody should have old items, but only one history should have each new item.
        #[allow(clippy::needless_range_loop)]
        for i in 0..COUNT {
            for j in 0..COUNT {
                // Old item.
                assert!(history_contains(&hists[i], texts[j]));

                // New item.
                let does_contain = history_contains(&hists[i], alt_texts[j]);
                let should_contain = i == j;
                assert_eq!(should_contain, does_contain);
            }
        }

        // Make sure incorporate_external_changes doesn't drop items! (#3496)
        let writer = &hists[0];
        let reader = &hists[1];
        let more_texts = [
            L!("Item_#3496_1"),
            L!("Item_#3496_2"),
            L!("Item_#3496_3"),
            L!("Item_#3496_4"),
            L!("Item_#3496_5"),
            L!("Item_#3496_6"),
        ];
        for i in 0..more_texts.len() {
            // time_barrier because merging will ignore items that may be newer
            if i > 0 {
                time_barrier();
            }
            writer.add_commandline(more_texts[i].to_owned());
            writer.incorporate_external_changes();
            reader.incorporate_external_changes();
            for text in more_texts.iter().take(i) {
                assert!(history_contains(reader, text));
            }
        }
        everything.clear();
    }

    #[test]
    fn test_history_path_detection() {
        // Regression test for #7582.
        // Temporary directory for the history files.
        let hist_tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(hist_tmpdir.path());

        // Temporary directory for the files we will detect.
        let tmpdir = fish_tempfile::new_dir().unwrap();

        // Place one valid file in the directory.
        let filename = L!("testfile");
        let file_path = tmpdir.path().join(filename.to_string());
        let wfile_path = WString::from(file_path.to_str().unwrap());
        std::fs::write(&file_path, []).unwrap();
        let wdir_path = WString::from(tmpdir.path().to_str().unwrap());

        let test_vars = EnvStack::new();
        let global_mode = EnvSetMode::new(EnvMode::GLOBAL, false);
        test_vars.set_one(L!("PWD"), global_mode, wdir_path.clone());
        test_vars.set_one(L!("HOME"), global_mode, wdir_path.clone());

        let history = create_test_history(L!("path_detection"), &hist_dir);
        history.clear();
        assert_eq!(history.size(), 0);
        history.add_pending_with_file_detection(
            L!("cmd0 not/a/valid/path"),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            &(L!("cmd1 ").to_owned() + filename),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            &(L!("cmd2 ").to_owned() + &wfile_path[..]),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            &(L!("cmd3  $HOME/").to_owned() + filename),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            L!("cmd4  $HOME/notafile"),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            &(L!("cmd5  ~/").to_owned() + filename),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            L!("cmd6  ~/notafile"),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            L!("cmd7  ~/*f*"),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.add_pending_with_file_detection(
            L!("cmd8  ~/*zzz*"),
            &test_vars,
            PersistenceMode::Disk,
        );
        history.resolve_pending();

        const HIST_SIZE: usize = 9;
        assert_eq!(history.size(), 9);

        // Expected sets of paths.
        let expected_paths = [
            vec![],                                   // cmd0
            vec![filename.to_owned()],                // cmd1
            vec![wfile_path],                         // cmd2
            vec![L!("$HOME/").to_owned() + filename], // cmd3
            vec![],                                   // cmd4
            vec![L!("~/").to_owned() + filename],     // cmd5
            vec![],                                   // cmd6
            vec![],                                   // cmd7 - we do not expand globs
            vec![],                                   // cmd8
        ];

        let maxlap = 128;
        for _lap in 0..maxlap {
            let mut failures = 0;
            for i in 1..=HIST_SIZE {
                if history.item_at_index(i).unwrap().get_required_paths()
                    != expected_paths[HIST_SIZE - i]
                {
                    failures += 1;
                }
            }
            if failures == 0 {
                break;
            }
            // The file detection takes a little time since it occurs in the background.
            // Loop until the test passes.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        history.clear();
    }

    #[test]
    fn test_history_formats() {
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        // Test reading legacy YAML history format directly.
        let yaml_file = workspace_root().join("tests/history_sample_fish_2_0");
        let contents = std::fs::read(yaml_file).unwrap();
        let mut items: Vec<WString> = yaml_compat::iterate_fish_2_0_history(&contents)
            .map(|item| item.str().to_owned())
            .collect();
        items.reverse(); // YAML is oldest-first, but we want newest-first
        let expected: Vec<WString> = vec![
            "echo this has\\\nbackslashes".into(),
            "function foo\necho bar\nend".into(),
            "echo alpha".into(),
        ];
        assert_eq!(items, expected);

        // Test bash import
        // The results are in the reverse order that they appear in the bash history file.
        // We don't expect whitespace to be elided (#4908: except for leading/trailing whitespace)
        let expected: Vec<WString> = vec![
            "EOF".into(),
            "sleep 123".into(),
            "posix_cmd_sub $(is supported but only splits on newlines)".into(),
            "posix_cmd_sub \"$(is supported)\"".into(),
            "a && echo valid construct".into(),
            "final line".into(),
            "echo supsup".into(),
            "export XVAR='exported'".into(),
            "history --help".into(),
            "echo foo".into(),
        ];
        let test_history_imported_from_bash = create_test_history(L!("bash_import"), &hist_dir);
        let file = std::fs::File::open(workspace_root().join("tests/history_sample_bash")).unwrap();
        test_history_imported_from_bash.populate_from_bash(BufReader::new(file));
        assert_eq!(test_history_imported_from_bash.get_history(), expected);
        test_history_imported_from_bash.clear();

        // Test reading corrupt YAML history - should handle gracefully.
        let corrupt_file = workspace_root().join("tests/history_sample_corrupt1");
        let contents = std::fs::read(corrupt_file).unwrap();
        let mut items: Vec<WString> = yaml_compat::iterate_fish_2_0_history(&contents)
            .map(|item| item.str().to_owned())
            .collect();
        items.reverse(); // YAML is oldest-first, but we want newest-first
        let expected: Vec<WString> = vec![
            "no_newline_at_end_of_file".into(),
            "corrupt_prefix".into(),
            "this_command_is_ok".into(),
        ];
        assert_eq!(items, expected);
    }

    #[test]
    fn test_history_item_cwd() {
        let tmpdir = fish_tempfile::new_dir().unwrap();
        let hist_dir = osstr2wcstring(tmpdir.path());

        let vars = EnvStack::new();
        let global_mode = EnvSetMode::new(EnvMode::GLOBAL, false);

        vars.set_one(L!("HOME"), global_mode, L!("/home/testuser").to_owned());

        // Regular path
        vars.set_one(L!("PWD"), global_mode, L!("/usr/local/bin").to_owned());
        let history = create_test_history(L!("test_cwd"), &hist_dir);
        history.add_pending_with_file_detection(L!("echo test1"), &vars, PersistenceMode::Disk);

        // Home directory path
        vars.set_one(
            L!("PWD"),
            global_mode,
            L!("/home/testuser/Documents").to_owned(),
        );
        history.add_pending_with_file_detection(L!("echo test2"), &vars, PersistenceMode::Disk);

        // Root directory
        vars.set_one(L!("PWD"), global_mode, L!("/").to_owned());
        history.add_pending_with_file_detection(L!("echo test3"), &vars, PersistenceMode::Disk);

        history.resolve_pending();

        assert_eq!(
            history.item_at_index(3).unwrap().cwd.as_deref(),
            Some(L!("/usr/local/bin"))
        );
        assert_eq!(
            history.item_at_index(2).unwrap().cwd.as_deref(),
            Some(L!("~/Documents"))
        );
        assert_eq!(
            history.item_at_index(1).unwrap().cwd.as_deref(),
            Some(L!("/"))
        );
    }
}
