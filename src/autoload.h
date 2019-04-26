// The classes responsible for autoloading functions and completions.
#ifndef FISH_AUTOLOAD_H
#define FISH_AUTOLOAD_H

#include <pthread.h>
#include <time.h>

#include <chrono>
#include <set>
#include <unordered_set>

#include "common.h"
#include "env.h"
#include "lru.h"
#include "wutil.h"

/// Record of an attempt to access a file.
struct file_access_attempt_t {
    /// If filled, the file ID of the checked file.
    /// Otherwise the file did not exist or was otherwise inaccessible.
    /// Note that this will never contain kInvalidFileID.
    maybe_t<file_id_t> file_id;

    /// When we last checked the file.
    time_t last_checked;

    /// Whether or not we believe we can access this file.
    bool accessible() const { return file_id.has_value(); }
};
file_access_attempt_t access_file(const wcstring &path, int mode);

struct autoload_function_t {
    autoload_function_t() = default;
    /// The last access attempt recorded
    file_access_attempt_t access{};
    /// Have we actually loaded this function?
    bool is_loaded{false};
    /// Whether we are a placeholder that stands in for "no such function". If this is true, then
    /// is_loaded must be false.
    bool is_placeholder{false};

    static autoload_function_t placeholder() {
        autoload_function_t result;
        result.is_placeholder = true;
        return result;
    }
};

class environment_t;

/// Class representing a path from which we can autoload and the autoloaded contents.
class autoload_t : private lru_cache_t<autoload_t, autoload_function_t> {
   private:
    friend lru_cache_t<autoload_t, autoload_function_t>;
    /// Lock for thread safety.
    std::mutex lock;
    /// The environment variable name.
    const wcstring env_var_name;
    /// The paths from which to autoload, or missing if none.
    maybe_t<wcstring_list_t> paths;
    /// A table containing all the files that are currently being loaded.
    /// This is here to help prevent recursion.
    std::unordered_set<wcstring> is_loading_set;

    void remove_all_functions() { this->evict_all_nodes(); }

    bool locate_file_and_maybe_load_it(const wcstring &cmd, bool really_load, bool reload,
                                       const wcstring_list_t &path_list);

    autoload_function_t *get_autoloaded_function_with_creation(const wcstring &cmd,
                                                               bool allow_eviction);

   public:
    // CRTP override
    void entry_was_evicted(wcstring key, autoload_function_t node);

    // Create an autoload_t for the given environment variable name.
    explicit autoload_t(wcstring env_var_name_var);

    /// Autoload the specified file, if it exists in the specified path. Do not load it multiple
    /// times unless its timestamp changes or parse_util_unload is called.
    /// @param cmd the filename to search for. The suffix '.fish' is always added to this name
    /// @param reload wheter to recheck file timestamps on already loaded files
    int load(const wcstring &cmd, bool reload);

    /// Check whether we have tried loading the given command. Does not do any I/O.
    bool has_tried_loading(const wcstring &cmd);

    /// Tell the autoloader that the specified file, in the specified path, is no longer loaded.
    /// Returns non-zero if the file was removed, zero if the file had not yet been loaded
    int unload(const wcstring &cmd);

    /// Check whether the given command could be loaded, but do not load it.
    bool can_load(const wcstring &cmd, const environment_t &vars);

    /// Invalidates all entries. Uesd when the underlying path variable changes.
    void invalidate();
};

/// Represents a file that we might want to autoload.
struct autoloadable_file_t {
    /// The path to the file.
    wcstring path;

    /// The metadata for the file.
    file_id_t file_id;
};

/// Class representing an autoloader observing a set of paths.
class autoload_observer_t {
    /// A timestamp is a monotonic point in time.
    using timestamp_t = std::chrono::time_point<std::chrono::steady_clock>;

    /// The directories from which to load.
    const wcstring_list_t dirs_;

    /// Our LRU cache of checks that were misses.
    /// The key is the command, the  value is the time of the check.
    struct misses_lru_cache_t : public lru_cache_t<misses_lru_cache_t, timestamp_t> {};
    misses_lru_cache_t misses_cache_;

    /// The set of files that we have returned to the caller, along with the time of the check.
    /// The key is the command (not the path).
    struct known_file_t {
        autoloadable_file_t file;
        timestamp_t last_checked;
    };
    std::unordered_map<wcstring, known_file_t> known_files_;

    /// \return the current timestamp.
    static timestamp_t current_timestamp() { return std::chrono::steady_clock::now(); }

    /// \return whether a timestamp is fresh enough to use.
    static bool is_fresh(timestamp_t then, timestamp_t now);

    /// Attempt to find an autoloadable file by searching our path list for a given comand.
    /// \return the file, or none() if none.
    maybe_t<autoloadable_file_t> locate_file(const wcstring &cmd) const;

   public:
    /// Initialize with a set of directories.
    explicit autoload_observer_t(wcstring_list_t dirs) : dirs_(std::move(dirs)) {}

    /// \return the directories.
    const wcstring_list_t &dirs() const { return dirs_; }

    /// Check if a command \p cmd can be loaded.
    /// If \p allow_stale is true, allow stale entries; otherwise discard them.
    /// This returns an autoloadable file, or none() if there is no such file.
    maybe_t<autoloadable_file_t> check(const wcstring &cmd, bool allow_stale = false);
};

#endif
