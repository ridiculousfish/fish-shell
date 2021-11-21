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
class history_item_t;

namespace histdb {

/// Holds the sqlite3 database connection.
struct history_db_conn_t;

/// Wraps the connection in a lock.
struct history_db_handle_t;
using history_db_handle_ref_t = std::shared_ptr<history_db_handle_t>;

/// Ways in which you can search history.
enum class search_mode_t : int {
    any,
    exact,
    contains,
    prefix,
    contains_glob,
    prefix_glob,
};

class search_t final : noncopyable_t, nonmovable_t {
   public:
    search_t(history_db_handle_ref_t handle, wcstring query, search_mode_t mode, bool icase)
        : handle_(handle), query_(std::move(query)), mode_(mode), icase_(icase) {}
    ~search_t();

    /// Access the current item, asserting we have one.
    const wcstring &current() const {
        assert(!items_.empty() && "No current item");
        return items_.back();
    }

    /// \return whether we have a current item.
    bool has_current() const { return !items_.empty(); }

    /// Advance to the next item.
    /// \return true if we have one, false if empty.
    /// This does NOT need to be called to get the first item.
    bool step() {
        if (items_.empty()) return false;
        items_.pop_back();
        if (items_.empty()) try_fill();
        return !items_.empty();
    }

   private:
    // Try filling our items.
    void try_fill();

    // List of items to return, with the next-up item at the end.
    wcstring_list_t items_{};

    // Last ID returned, used for windowing.
    int64_t last_id_{INT64_MAX};

    // Our DB handle.
    const history_db_handle_ref_t handle_;

    // Properties of the search.
    const wcstring query_;
    const search_mode_t mode_;
    const bool icase_;

    friend class history_db_t;
    friend struct history_db_conn_t;
};

/// Our wrapper around SQLite.
class history_t;
class search_t;
class history_db_t : noncopyable_t, nonmovable_t {
   public:
    /// Attempt to open a DB file at the given path, creating it if it does not exist.
    /// \return the file, or nullptr on failure in which case an error will have been logged.
    static std::unique_ptr<history_db_t> create_at_path(const wcstring &path);

    /// Add an item to history.
    void add(const history_item_t &item);

    /// Construct a history search.
    std::unique_ptr<search_t> search(const wcstring &query, search_mode_t mode, bool icase) const;

    /// Construct a history "search" that just enumerates all items.
    std::unique_ptr<search_t> list() const { return this->search(L"", search_mode_t::any, false); }

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
