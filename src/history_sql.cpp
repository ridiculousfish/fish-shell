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
#include "wcstringutil.h"
#include "wildcard.h"

extern "C" {
#include "sha3.h"
}

namespace {

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

/// \return true if a history item matches a given search query.
static bool text_matches_search(histdb::search_mode_t mode, const wcstring &query,
                                const wcstring &text, bool icase) {
    using histdb::search_mode_t;
    assert(!icase && "icase not yet supported");
    switch (mode) {
        case search_mode_t::exact:
            return query == text;

        case search_mode_t::contains:
            return text.find(query) != wcstring::npos;

        case search_mode_t::prefix:
            return string_prefixes_string(query, text);

        case search_mode_t::contains_glob: {
            wcstring wcpattern1 = parse_util_unescape_wildcards(query);
            if (wcpattern1.front() != ANY_STRING) wcpattern1.insert(0, 1, ANY_STRING);
            if (wcpattern1.back() != ANY_STRING) wcpattern1.push_back(ANY_STRING);
            return wildcard_match(text, wcpattern1);
        }

        case search_mode_t::prefix_glob: {
            wcstring wcpattern2 = parse_util_unescape_wildcards(query);
            if (wcpattern2.back() != ANY_STRING) wcpattern2.push_back(ANY_STRING);
            return wildcard_match(text, wcpattern2);
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
    sqlite3_stmt *stmt{};

    // Per SQLite: "The application must finalize every prepared statement in order to avoid
    // resource leaks."
    void finalize() {
        // Note this only returns errors from evaluating the statement; ignore those.
        (void)sqlite3_finalize(stmt);
        stmt = nullptr;
    }

    // Reset this statement.
    int reset() { return sqlite3_reset(this->stmt); }

   protected:
    // Bind a string to a parameter index.
    // \return any SQLite error.
    int bind_str(int idx, const std::string &s) {
        return sqlite3_bind_text(this->stmt, idx, s.c_str(), (int)s.size(), SQLITE_TRANSIENT);
    }

    // Bind an int to a parameter index.
    // \return any SQLite error.
    int bind_int(int idx, sqlite_int64 val) { return sqlite3_bind_int64(this->stmt, idx, val); }

    ~prepared_statement_t() { assert(!stmt && "Statement was not finalized"); }
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
        "SELECT MAX(items.id), texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    WHERE items.id < :max_id"
        "    GROUP BY items.text_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        max_id_param_idx = 1,
        amount_param_idx,
    };
};

// Query used for fuzzy history search.
// Mode can be contains, prefix, contains_glob, prefix_glob, optionally icase.
struct search_items_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "SELECT MAX(items.id), texts.contents"
        "    FROM items"
        "    INNER JOIN texts ON texts.id = items.text_id"
        "    WHERE items.id < :max_id"
        "    AND histmatch(:mode, :icase, :query, texts.contents)"
        "    GROUP BY items.text_id"
        "    ORDER BY items.id DESC"
        "    LIMIT :amount";

    enum {
        max_id_param_idx = 1,
        mode_param_idx,
        icase_param_idx,
        query_param_idx,
    };
};

}  // namespace sql
}  // namespace

namespace histdb {

struct history_db_conn_t : noncopyable_t {
    explicit history_db_conn_t(wcstring path) : path_(path) {}

    // history_db_conn_t is "movable" to allow construction, but must NOT be moved after
    // initialize() is called.
    history_db_conn_t(history_db_conn_t &&) = default;

    ~history_db_conn_t() {
        // Free our prepared statements.
        ensure_content.finalize();
        insert_item.finalize();
        get_items.finalize();
        search_items.finalize();

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
        if (check_fail(__LINE__, sqlite3_open_v2(path.c_str(), &this->db, flags, nullptr)))
            return false;
        if (check_fail(__LINE__, sqlite3_busy_timeout(this->db, 250 /* ms */))) return false;
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
        if (check_fail(__LINE__,
                       sqlite3_create_function(this->db, "sha3_prefix64", 1,
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

        int search_mode = sqlite3_value_int(argv[0]);
        bool icase = sqlite3_value_int(argv[1]);

        const unsigned char *query = sqlite3_value_text(argv[2]);
        int query_len = sqlite3_value_bytes(argv[2]);
        if (!query || query_len < 0) return;

        const unsigned char *text = sqlite3_value_text(argv[3]);
        int text_len = sqlite3_value_bytes(argv[3]);
        if (!text || text_len < 0) return;

        wcstring wquery =
            str2wcstring(reinterpret_cast<const char *>(query), static_cast<size_t>(query_len));
        wcstring wtext =
            str2wcstring(reinterpret_cast<const char *>(text), static_cast<size_t>(text_len));

        bool matches = text_matches_search(static_cast<histdb::search_mode_t>(search_mode), wquery,
                                           wtext, icase);
        sqlite3_result_int(ctx, matches ? 1 : 0);
    }

    // Create our history match function.
    bool install_histmatch_function() {
        if (check_fail(__LINE__, sqlite3_create_function(
                                     this->db, "histmatch", 4, SQLITE_UTF8 | SQLITE_DETERMINISTIC,
                                     nullptr, sql_histmatch, nullptr, nullptr))) {
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
        if (!prepare_1_stmt(search_items.SQL, &this->search_items)) return false;
        return true;
    }

    /// Prepare a single statement, logging on error.
    bool prepare_1_stmt(const char *sql, sql::prepared_statement_t *ps) {
        // Asks SQLite to compute string length for us.
        const int nbyte = -1;
        if (check_fail(__LINE__, sqlite3_prepare_v2(this->db, sql, nbyte, &ps->stmt, nullptr))) {
            FLOG(error, "SQL is:", sql);
            return false;
        }
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
            if (check_fail(__LINE__, ps.reset())) return false;
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
        if (check_fail(__LINE__, ensure_content.bind(storage))) return false;
        if (check_fail(__LINE__, run_stmt(ensure_content))) return false;

        // Add the item.
        if (check_fail(__LINE__, insert_item.bind(storage, item.timestamp()))) return false;
        if (check_fail(__LINE__, run_stmt(insert_item))) return false;
        return true;
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
    sql::get_items_t get_items{};

    // Finds items matching a query, before a given item.
    sql::search_items_t search_items{};

    // String storage.
    std::string storage;
};

struct search_impl_t final : public search_t {
    explicit search_impl_t(history_db_handle_ref_t handle) : handle_(handle) {}
    const history_db_handle_ref_t handle_;
    void try_fill() override;

    ~search_impl_t() = default;
};

void search_impl_t::try_fill() {}

search_t::~search_t() = default;

struct history_db_handle_t {
    owning_lock<history_db_conn_t> lock;
    explicit history_db_handle_t(const wcstring &path) : lock(history_db_conn_t(path)) {}
};

// static
std::unique_ptr<history_db_t> history_db_t::create_at_path(const wcstring &path) {
    std::unique_ptr<history_db_t> hist{
        new history_db_t(std::make_shared<history_db_handle_t>(path))};
    if (!hist->conn()->initialize()) return nullptr;
    return hist;
}

acquired_lock<history_db_conn_t> history_db_t::conn() { return handle_->lock.acquire(); }

void history_db_t::add_from(::history_t *hist) { conn()->add_from(hist); }

std::unique_ptr<search_t> history_db_t::search(const wcstring &query, search_mode_t mode,
                                               bool icase) const {
    return make_unique<search_impl_t>(this->handle_);
}

history_db_t::~history_db_t() = default;
}  // namespace histdb

#endif
