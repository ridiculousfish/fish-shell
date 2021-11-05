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

/// Our wrapper around SQLite.
class history_t;
class history_db_t : noncopyable_t, nonmovable_t {
   public:
    /// Attempt to open a DB file at the given path, creating it if it does not exist.
    /// \return the file, or nullptr on failure in which case an error will have been logged.
    static std::unique_ptr<history_db_t> create_at_path(const wcstring &path);

    // Temporary hack.
    virtual void add_from(history_t *hist) = 0;

    virtual ~history_db_t();
};

#endif
#endif
