#include "config.h"

#include "history_sql.h"

#include <cstring>

#include "fds.h"
#include "flog.h"
#include "history.h"
#include "path.h"
#include "sqlite3.h"
#include "utf8.h"

namespace {

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
    int reset() {
        return sqlite3_reset(this->stmt);
    }

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
        if (! ret) ret = this->bind_str(TEXT_PARAM_IDX, text);
        return ret;
    }
};

// Insert a new history item.
struct insert_item_t : public prepared_statement_t {
    static constexpr const char *SQL =
        "INSERT INTO items(text_id, timestamp) "
        "  SELECT id, ?timestamp FROM texts "
        "  WHERE contents = :text "
        "  LIMIT 1 ";

    static constexpr int TIMESTAMP_PARAM_IDX = 1;
    static constexpr int TEXT_PARAM_IDX = 2;

    // Reset and bind our parameters.
    int bind(const std::string &text, time_t timestamp) {
        int ret = this->reset();
        if (! ret) ret = this->bind_int(TIMESTAMP_PARAM_IDX, timestamp);
        if (! ret) ret = this->bind_str(TEXT_PARAM_IDX, text);
        return ret;
    }
};
}  // namespace sql

struct history_db_impl_t final : public history_db_t {
    ~history_db_impl_t() {
        // Free our prepared statements.
        ensure_content.finalize();
        insert_item.finalize();

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
                FLOG(error, "SQL failed:", this->error());
                return true;
        }
    }

    // Initialize this db, constructing the prepared statements and creating the table.
    // \return true on success, false on failure.
    bool initialize(const wcstring &wpath) {
        assert(!this->db && "Already initialized");
        std::string path = wcs2string(wpath);
        int flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX;
        if (check_fail(__LINE__, sqlite3_open_v2(path.c_str(), &this->db, flags, nullptr))) return false;
        if (check_fail(__LINE__, sqlite3_busy_timeout(this->db, 250 /* ms */))) return false;
        if (check_fail_sql("PRAGMA synchronous = NORMAL")) return false;
        if (check_fail_sql(sql::CREATE_TABLES)) return false;
        if (!this->prepare()) return false;
        return true;
    }

    // Construct our prepared statements.
    // \return whether we succeeded.
    bool prepare() {
        if (!prepare_1_stmt(ensure_content.SQL, &this->ensure_content)) return false;
        if (!prepare_1_stmt(insert_item.SQL, &this->insert_item)) return false;
        return true;
    }

    /// Prepare a single statement, logging on error.
    bool prepare_1_stmt(const char *sql, sql::prepared_statement_t *ps) {
        // Asks SQLite to compute string length for us.
        const int nbyte = -1;
        if (check_fail(__LINE__, sqlite3_prepare_v2(this->db, sql, nbyte, &ps->stmt, nullptr))) {
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

    // Add a history item.
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

    void add_from(history_t *hist) override {
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

    // Our SQLite connection.
    sqlite3 *db{};

    // Prepared statements.
    // Adds a history text into the contents table, if not already present.
    sql::ensure_content_t ensure_content{};

    // Adds a new item into the item table.
    sql::insert_item_t insert_item{};

    // String storage.
    std::string storage;
};
}  // namespace

// static
std::unique_ptr<history_db_t> history_db_t::create_at_path(const wcstring &path) {
    auto hist = make_unique<history_db_impl_t>();
    if (!hist->initialize(path)) return nullptr;
    return hist;
}

history_db_t::~history_db_t() = default;
