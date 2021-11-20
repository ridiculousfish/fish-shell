#ifndef FISH_HISTORY_SQL_H
#define FISH_HISTORY_SQL_H

#include "config.h"

#include <sys/mman.h>

#include <cassert>
#include <ctime>
#include <memory>

#include "common.h"
#include "maybe.h"

#define FISH_HISTORY_SQL 1

#if FISH_HISTORY_SQL

class history_t;

namespace histdb {

/// Holds the sqlite3 database connection.
struct history_db_conn_t;

/// Wraps the connection in a lock.
struct history_db_handle_t;
using history_db_handle_ref_t = std::shared_ptr<history_db_handle_t>;

/// Ways in which you can search history.
enum class search_mode_t {
    exact,
    contains,
    prefix,
    contains_glob,
    prefix_glob,
    everything,
};

class search_t : noncopyable_t, nonmovable_t {
   public:
    virtual ~search_t();

    // Access the current item.
    const maybe_t<wcstring> &current() const { return current_; }

    /// Advance to the next item.
    /// \return true if we get one, false if empty.
    virtual bool step() const;

   private:
    maybe_t<wcstring> current_{};
};

/// Our wrapper around SQLite.
class history_t;
class search_t;
class history_db_t : noncopyable_t, nonmovable_t {
   public:
    /// Attempt to open a DB file at the given path, creating it if it does not exist.
    /// \return the file, or nullptr on failure in which case an error will have been logged.
    static std::unique_ptr<history_db_t> create_at_path(const wcstring &path);

    /// Construct a history search.
    std::unique_ptr<search_t> search(const wcstring &query, search_mode_t mode, bool icase) const;

    // Temporary hack.
    void add_from(::history_t *hist);

    ~history_db_t();

   private:
    history_db_t(history_db_handle_ref_t handle) : handle_(std::move(handle)) {}

    acquired_lock<history_db_conn_t> conn();
    history_db_handle_ref_t handle_;
};
}  // namespace histdb
#endif
#endif
