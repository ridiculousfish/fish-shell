#include "config.h"

#include "history_sql.h"

#if FISH_HISTORY_SQL

#include <cstring>

#include "fds.h"
#include "flog.h"
#include "history.h"
#include "parse_util.h"
#include "path.h"
#include "sqlite3.h"
#include "utf8.h"
#include "wildcard.h"

extern "C" {
#include "sha3.h"
}

namespace histdb {
namespace {

// The number of items that a history search will return in a "window" (from a single query).
constexpr size_t HISTORY_SEARCH_WINDOW_SIZE = 24;

// Helper to invoke check_fail, propagating the line number.
#define SQLCHECK(x) check_fail(__LINE__, (x))

// SQLite3 strings are unsigned.
wcstring sqlstr2wcstr(const unsigned char *str) {
    if (!str) return wcstring{};
    return str2wcstring(reinterpret_cast<const char *>(str));
}

wcstring sqlstr2wcstr(const unsigned char *str, int len) {
    if (!str || len <= 0) return wcstring{};
    return str2wcstring(reinterpret_cast<const char *>(str), static_cast<size_t>(len));
}

// SQLite plugin function that hashes a string using sha256, returning the first 8 bytes as an
// int64.
int64_t sha3_prefix_hash(const unsigned char *str, size_t len) {
    uint8_t sha[64];
    sha3(str, len, sha, sizeof sha);
    uint64_t result = 0;
    for (size_t i = 0; i < 8; i++) {
        result = result << 8 | sha[i];
    }
    return *reinterpret_cast<int64_t *>(&result);
}

/// \return true if a history item matches a given search query. If icase is set, the query is
/// already lowercased.
static bool text_matches_search(search_mode_t mode, const wcstring &query, const wcstring &inp_text,
                                bool icase) {
    wcstring text_lower;
    if (icase) {
        text_lower = wcstolower(inp_text);
    }
    const wcstring &eff_text = icase ? text_lower : inp_text;

    switch (mode) {
        case search_mode_t::any:
            return true;

        case search_mode_t::exact:
            return query == eff_text;

        case search_mode_t::contains:
            return eff_text.find(query) != wcstring::npos;

        case search_mode_t::prefix:
            return string_prefixes_string(query, eff_text);

        case search_mode_t::contains_glob: {
            wcstring wcpattern1 = parse_util_unescape_wildcards(query);
            if (wcpattern1.front() != ANY_STRING) wcpattern1.insert(0, 1, ANY_STRING);
            if (wcpattern1.back() != ANY_STRING) wcpattern1.push_back(ANY_STRING);
            return wildcard_match(eff_text, wcpattern1);
        }

        case search_mode_t::prefix_glob: {
            wcstring wcpattern2 = parse_util_unescape_wildcards(query);
            if (wcpattern2.back() != ANY_STRING) wcpattern2.push_back(ANY_STRING);
            return wildcard_match(eff_text, wcpattern2);
        }

        default:
            DIE("Unknown search mode");
    }
}

namespace sql {

// Create our tables.
// Note this is NOT run as a prepared statement.
// const char *CREATE_TABLES_SQL =
//    "CREATE TABLE IF NOT EXISTS texts ("
//    "id INTEGER PRIMARY KEY,"
//    "contents TEXT NOT NULL,"
//    "contents_hash INTEGER NOT NULL"
//    ")"
//    ""
//    "CREATE INDEX contents_hash_idx"
//    "on texts(contents_hash);"
//    ""
//    ""
//    "CREATE TABLE IF NOT EXISTS items ("
//    "text_id INTEGER NOT NULL,"
//    "timestamp INTEGER NOT NULL,"
//    "FOREIGN KEY (text_id) REFERENCES texts (id)"
//    "ON DELETE CASCADE"
//    ");";

constexpr const char *CREATE_TABLES =
    "CREATE TABLE IF NOT EXISTS texts ( "
    "id INTEGER PRIMARY KEY, "
    "contents TEXT NOT NULL UNIQUE "
    ");"
    ""
    "CREATE INDEX IF NOT EXISTS contents_idx "
    "on texts(contents); "
    ""
    ""
    "CREATE TABLE IF NOT EXISTS items ( "
    "id INTEGER PRIMARY KEY, "
    "text_id INTEGER NOT NULL, "
    "timestamp INTEGER NOT NULL, "
    "FOREIGN KEY (text_id) REFERENCES texts (id) "
    "ON DELETE CASCADE "
    ");";

// A prepared statement
struct prepared_statement_t {
    // This is finalized by history_db_conn_t.
    sqlite3_stmt *stmt{};

    // Reset this statement.
    int reset() { return sqlite3_reset(this->stmt); }

   protected:
    // Bind a string to a parameter index.
    // \return any SQLite error.
    int bind_str(int idx, const std::string &s) {
        return sqlite3_bind_text(this->stmt, idx, s.c_str(), (int)s.size(), SQLITE_TRANSIENT);
    }
    int bind_str(int idx, const wcstring &s) { return bind_str(idx, wcs2string(s)); }

    // Bind an int to a parameter index.
    // \return any SQLite error.
    int bind_int(int idx, sqlite_int64 val) { return sqlite3_bind_int64(this->stmt, idx, val); }
};

// Ensure a history text is present.
struct ensure_content_t : public prepared_statement_t {
    static constexpr const char *SQL = "INSERT OR IGNORE INTO texts(contents) VALUES (:text);";
    static constexpr int TEXT_PARAM_IDX = 1;

    // Reset and bind our parameters.
    int bind(const std::string &text) {
        int ret = this->reset();
        if (!ret) ret = this->bind_str(TEXT_PARAM_IDX, text);
        return ret;
    }
};

// Insert a new history item.
struct insert_item_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "INSERT INTO items(text_id, timestamp) "
        "  SELECT id, :timestamp FROM texts "
        "  WHERE contents = :text "
        "  LIMIT 1 ";

    enum {
        timestamp_param_idx = 1,
        text_param_idx,
    };

    // Reset and bind our parameters.
    int bind(const std::string &text, time_t timestamp) {
        int ret = this->reset();
        if (!ret) ret = this->bind_int(timestamp_param_idx, timestamp);
        if (!ret) ret = this->bind_str(text_param_idx, text);
        return ret;
    }
};

// Find distinct items before a given item.
struct get_items_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "SELECT items.id, items.text_id, items.timestamp, texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    WHERE items.id < :max_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        max_id_param_idx = 1,
        amount_param_idx,
    };

    // Reset and bind our parameters.
    int bind(sqlite3_int64 max_id, int amount) {
        int ret = this->reset();
        if (!ret) ret = this->bind_int(max_id_param_idx, max_id);
        if (!ret) ret = this->bind_int(amount_param_idx, amount);
        return ret;
    }
};

// Find distinct items before a given item.
struct get_items_distinct_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "SELECT MAX(items.id) as max_id, items.text_id, items.timestamp, texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    GROUP BY items.text_id"
        "    HAVING max_id < :max_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        max_id_param_idx = 1,
        amount_param_idx,
    };

    // Reset and bind our parameters.
    int bind(sqlite3_int64 max_id, int amount) {
        int ret = this->reset();
        if (!ret) ret = this->bind_int(max_id_param_idx, max_id);
        if (!ret) ret = this->bind_int(amount_param_idx, amount);
        return ret;
    }
};

// Query used for fuzzy history search.
// Mode can be contains, prefix, contains_glob, prefix_glob, optionally icase.
struct search_items_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "SELECT items.id, items.text_id, items.timestamp, texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    AND histmatch(:mode, :icase, :query, texts.contents)"
        "    WHERE items.id < :max_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        mode_param_idx = 1,
        icase_param_idx,
        query_param_idx,
        max_id_param_idx,
        amount_param_idx,
    };

    // Reset and bind our parameters.
    int bind(sqlite3_int64 max_id, search_mode_t mode, bool icase, const wcstring &query,
             int amount) {
        int ret = this->reset();
        if (!ret) ret = this->bind_int(max_id_param_idx, max_id);
        if (!ret) ret = this->bind_int(mode_param_idx, static_cast<int>(mode));
        if (!ret) ret = this->bind_int(icase_param_idx, static_cast<int>(icase));
        if (!ret) ret = this->bind_str(query_param_idx, query);
        if (!ret) ret = this->bind_int(amount_param_idx, amount);
        return ret;
    }
};

// Query used for fuzzy history search.
// Mode can be contains, prefix, contains_glob, prefix_glob, optionally icase.
struct search_items_distinct_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "SELECT MAX(items.id) as max_id, items.text_id, items.timestamp, texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    AND histmatch(:mode, :icase, :query, texts.contents)"
        "    GROUP BY items.text_id"
        "    HAVING max_id < :max_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        mode_param_idx = 1,
        icase_param_idx,
        query_param_idx,
        max_id_param_idx,
        amount_param_idx,
    };

    // Reset and bind our parameters.
    int bind(sqlite3_int64 max_id, search_mode_t mode, bool icase, const wcstring &query,
             int amount) {
        int ret = this->reset();
        if (!ret) ret = this->bind_int(max_id_param_idx, max_id);
        if (!ret) ret = this->bind_int(mode_param_idx, static_cast<int>(mode));
        if (!ret) ret = this->bind_int(icase_param_idx, static_cast<int>(icase));
        if (!ret) ret = this->bind_str(query_param_idx, query);
        if (!ret) ret = this->bind_int(amount_param_idx, amount);
        return ret;
    }
};

}  // namespace sql
}  // namespace

struct history_db_conn_t : noncopyable_t {
    explicit history_db_conn_t(wcstring path) : path_(path) {}

    // history_db_conn_t is "movable" to allow construction, but must NOT be moved after
    // initialize() is called.
    history_db_conn_t(history_db_conn_t &&) = default;

    ~history_db_conn_t() {
        // Per SQLite: "The application must finalize every prepared statement in order to avoid
        // resource leaks."
        for (auto *stmt : finalizees_) sqlite3_finalize(stmt);
        // sqlite3_close is null-safe.
        sqlite3_close(this->db);
    }

    // \return the most recent error message.
    wcstring error() const { return str2wcstring(sqlite3_errmsg(this->db)); }

    // Given an error code, print an error if it's not OK.
    // \return true on failure, false on success.
    bool check_fail(int line, int sqlite_err) const {
        switch (sqlite_err) {
            case SQLITE_OK:
                return false;

            case SQLITE_MISUSE:
                // An error message is not set in this case.
                FLOG(error, "SQLite misuse from line", line);
                return true;

            default:
                FLOG(error, "SQL failed from line", line, "with error:", this->error());
                return true;
        }
    }

    // Initialize this db, constructing the prepared statements and creating the table.
    // \return true on success, false on failure.
    bool initialize() {
        assert(!this->db && "Already initialized");
        std::string path = wcs2string(path_);
        int flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX;
        if (SQLCHECK(sqlite3_open_v2(path.c_str(), &this->db, flags, nullptr))) return false;
        if (SQLCHECK(sqlite3_busy_timeout(this->db, 250 /* ms */))) return false;
        if (check_fail_sql("PRAGMA synchronous = NORMAL")) return false;
        if (!this->install_histmatch_function()) return false;
        if (!this->install_sha3_function()) return false;
        if (check_fail_sql(sql::CREATE_TABLES)) return false;
        if (!this->prepare()) return false;
        return true;
    }

    // Our sha3 hash function, installed as "sha3_prefix64" in SQLite.
    static void sql_sha3_prefix(sqlite3_context *ctx, int argc, sqlite3_value **argv) {
        if (argc != 1) {
            FLOG(error, "sha3_prefix64() called with wrong number of arguments");
            return;
        }
        const unsigned char *text = sqlite3_value_text(argv[0]);
        int len = sqlite3_value_bytes(argv[0]);
        if (text && len >= 0) {
            sqlite3_result_int64(ctx, sha3_prefix_hash(text, len));
        }
    }

    // Create our sha3 hash function.
    bool install_sha3_function() {
        if (SQLCHECK(sqlite3_create_function(this->db, "sha3_prefix64", 1,
                                             SQLITE_UTF8 | SQLITE_DETERMINISTIC, nullptr,
                                             sql_sha3_prefix, nullptr, nullptr))) {
            return false;
        }
        return true;
    }

    // Our "histmatch" function takes the search mode as an int, icase as an int, the query string,
    // and the text to match.
    static void sql_histmatch(sqlite3_context *ctx, int argc, sqlite3_value **argv) {
        if (argc != 4) {
            FLOG(error, "histmatch() called with wrong number of arguments");
            return;
        }

        sqlite3_value *mode_arg = argv[0];
        sqlite3_value *icase_arg = argv[1];
        sqlite3_value *query_arg = argv[2];
        sqlite3_value *text_arg = argv[3];

        int search_mode = sqlite3_value_int(mode_arg);
        assert(search_mode >= 0 && search_mode <= static_cast<int>(search_mode_t::prefix_glob) &&
               "Invalid search mode");

        bool icase = sqlite3_value_int(icase_arg);

        const unsigned char *query = sqlite3_value_text(query_arg);
        int query_len = sqlite3_value_bytes(query_arg);
        if (!query || query_len < 0) return;

        const unsigned char *text = sqlite3_value_text(text_arg);
        int text_len = sqlite3_value_bytes(text_arg);
        if (!text || text_len < 0) return;

        bool matches = text_matches_search(static_cast<histdb::search_mode_t>(search_mode),
                                           sqlstr2wcstr(query, query_len),
                                           sqlstr2wcstr(text, text_len), icase);
        sqlite3_result_int(ctx, matches ? 1 : 0);
    }

    // Create our history match function.
    bool install_histmatch_function() {
        if (SQLCHECK(sqlite3_create_function(this->db, "histmatch", 4,
                                             SQLITE_UTF8 | SQLITE_DETERMINISTIC, nullptr,
                                             sql_histmatch, nullptr, nullptr))) {
            return false;
        }
        return true;
    }

    // Construct our prepared statements.
    // \return whether we succeeded.
    bool prepare() {
        if (!prepare_1_stmt(ensure_content.SQL, &this->ensure_content)) return false;
        if (!prepare_1_stmt(insert_item.SQL, &this->insert_item)) return false;
        if (!prepare_1_stmt(get_items.SQL, &this->get_items)) return false;
        if (!prepare_1_stmt(get_items_distinct.SQL, &this->get_items_distinct)) return false;
        if (!prepare_1_stmt(search_items.SQL, &this->search_items)) return false;
        if (!prepare_1_stmt(search_items_distinct.SQL, &this->search_items_distinct)) return false;
        return true;
    }

    /// Prepare a single statement, logging on error.
    bool prepare_1_stmt(const char *sql, sql::prepared_statement_t *ps) {
        // Asks SQLite to compute string length for us.
        const int nbyte = -1;
        if (SQLCHECK(sqlite3_prepare_v2(this->db, sql, nbyte, &ps->stmt, nullptr))) {
            FLOG(error, "SQL is:", sql);
            return false;
        }
        finalizees_.push_back(ps->stmt);
        return true;
    }

    // Run some SQL, logging on error.
    // \return true on failure.
    bool check_fail_sql(const char *sql) {
        FLOG(history_sql, sql);
        char *errmsg = nullptr;
        int ret =
            sqlite3_exec(this->db, sql, nullptr /* callback */, nullptr /* context */, &errmsg);
        if (ret != SQLITE_OK) {
            FLOGF(error, "SQL failed: %s", errmsg);
            sqlite3_free(errmsg);
        }
        return ret != SQLITE_OK;
    }

    // Run a prepared statement which returns no data.
    // \return a SQLite error code.
    template <typename PS>
    int run_stmt(PS &ps) {
        FLOGF(history_sql, "%s", PS::SQL);
        assert(ps.stmt && "Null statement");
        // Loop on BUSY.
        int ret;
        do {
            if (SQLCHECK(ps.reset())) return false;
            ret = sqlite3_step(ps.stmt);
        } while (ret == SQLITE_BUSY);
        assert(ret != SQLITE_ROW && "Should not get row data from query");
        return ret == SQLITE_DONE ? SQLITE_OK : ret;
    }

    // Add a history item. Note this is run inside a transaction.
    bool add_item(const history_item_t &item) {
        if (!wchar_to_utf8_string(item.str(), &storage)) {
            FLOG(error, "Unable to encode history item");
            return false;
        }

        // Ensure we have text content in the DB.
        if (SQLCHECK(ensure_content.bind(storage))) return false;
        if (SQLCHECK(run_stmt(ensure_content))) return false;

        // Add the item.
        if (SQLCHECK(insert_item.bind(storage, item.timestamp()))) return false;
        if (SQLCHECK(run_stmt(insert_item))) return false;
        return true;
    }

    void add(const history_item_t &item) {
        check_fail_sql("BEGIN");
        add_item(item);
        check_fail_sql("COMMIT");
    }

    void add_from(::history_t *hist) {
        check_fail_sql("BEGIN");
        for (size_t i = 1, max = hist->size(); i <= max; i++) {
            history_item_t item = hist->item_at_index(i);
            if (item.empty()) {
                FLOG(error, "Empty item at index", i);
                continue;
            }
            if (!add_item(item)) break;
        }
        check_fail_sql("COMMIT");
    }

    // Add items to the given search.
    void fill_search(search_t *search) {
        assert(search && "Null search");
        assert(search->items_.empty() && "Items should be empty if filling");
        // Decide which prepared statement to use.
        // If the search is just matching everything we can be more efficient.
        bool distinct = !(search->flags_ & history_search_no_dedup);
        sql::prepared_statement_t *ps{};
        if (search->mode_ == search_mode_t::any && !distinct) {
            if (SQLCHECK(get_items.bind(search->last_id_, HISTORY_SEARCH_WINDOW_SIZE))) {
                return;
            }
            ps = &get_items;
        } else if (search->mode_ == search_mode_t::any /* && distinct */) {
            if (SQLCHECK(get_items_distinct.bind(search->last_id_, HISTORY_SEARCH_WINDOW_SIZE))) {
                return;
            }
            ps = &get_items_distinct;
        } else if (!distinct) {
            if (SQLCHECK(search_items.bind(search->last_id_, search->mode_,
                                           search->flags_ & history_search_ignore_case,
                                           search->query_canon_, HISTORY_SEARCH_WINDOW_SIZE))) {
                return;
            }
            ps = &search_items;
        } else /* distinct */ {
            if (SQLCHECK(search_items_distinct.bind(
                    search->last_id_, search->mode_, search->flags_ & history_search_ignore_case,
                    search->query_canon_, HISTORY_SEARCH_WINDOW_SIZE))) {
                return;
            }
            ps = &search_items_distinct;
        }

        // Fields which our query has selected.
        enum result_idxs {
            col_id,
            col_text_id,
            col_timestamp,
            col_contents,
        };

        // Start fetching our items.
        // Loop on BUSY.
        bool done = false;
        while (!done) {
            int ret = sqlite3_step(ps->stmt);
            switch (ret) {
                case SQLITE_ROW: {
                    int64_t id = sqlite3_column_int64(ps->stmt, col_id);
                    int timestamp = sqlite3_column_int(ps->stmt, col_timestamp);
                    wcstring contents = sqlstr2wcstr(sqlite3_column_text(ps->stmt, col_contents));
                    (void)timestamp;
                    search->items_.push_back(std::move(contents));
                    search->last_id_ = std::min(search->last_id_, id);
                    break;
                }
                case SQLITE_DONE:
                    done = true;
                    break;
                case SQLITE_BUSY:
                    continue;
                default:
                    SQLCHECK(ret);
                    search->items_.clear();
                    return;
            }
        }

        // We have added a bunch of items with the oldest item at the end.
        // We want the oldest item at the beginning.
        std::reverse(search->items_.begin(), search->items_.end());
    }

    // Path to the file on disk, or empty for in-memory.
    const wcstring path_;

    // Our SQLite connection.
    sqlite3 *db{};

    // Prepared statements.
    // Adds a history text into the contents table, if not already present.
    sql::ensure_content_t ensure_content{};

    // Adds a new item into the item table.
    sql::insert_item_t insert_item{};

    // Finds items before a given item.
    // The "distinct" version deduplicates but is more expensive.
    sql::get_items_t get_items{};
    sql::get_items_distinct_t get_items_distinct{};

    // Finds items matching a query, before a given item.
    sql::search_items_t search_items{};
    sql::search_items_distinct_t search_items_distinct{};

    // List of prepared statements that we are responsible for finalizing.
    std::vector<sqlite3_stmt *> finalizees_;

    // String storage.
    std::string storage;
};

struct history_db_handle_t {
    owning_lock<history_db_conn_t> lock;
    explicit history_db_handle_t(const wcstring &path) : lock(history_db_conn_t(path)) {}
};

search_t::~search_t() = default;

void search_t::try_fill() { handle_->lock.acquire()->fill_search(this); }

// static
std::unique_ptr<history_db_t> history_db_t::create_at_path(const wcstring &path) {
    std::unique_ptr<history_db_t> hist{
        new history_db_t(std::make_shared<history_db_handle_t>(path))};
    if (!hist->conn()->initialize()) return nullptr;
    return hist;
}

acquired_lock<history_db_conn_t> history_db_t::conn() { return handle_->lock.acquire(); }

void history_db_t::add(const history_item_t &item) { conn()->add(item); }

void history_db_t::add_from(::history_t *hist) { conn()->add_from(hist); }

std::unique_ptr<search_t> history_db_t::search(const wcstring &query, search_mode_t mode,
                                               history_search_flags_t flags) const {
    auto search = make_unique<search_t>(this->handle_, query, mode, flags);
    search->try_fill();
    return search;
}

history_db_t::~history_db_t() = default;
}  // namespace histdb

#endif
