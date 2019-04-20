/// The flogger: debug logging support for fish.
#ifndef FISH_FLOG_H
#define FISH_FLOG_H

#include "config.h"  // IWYU pragma: keep

#include "enum_set.h"

#include <string>
#include <type_traits>

/// These are the categories of logs that fish may emit.
enum class fish_log_category_t {
    /// Log a profound failure. This is on by default.
    ohno,

    /// Log for debugging. This is on by default.
    debug,

    COUNT
};

using wcstring = std::wstring;
void set_flog_categories_pattern(const wcstring &str);

namespace flog_details {
bool should_flog(fish_log_category_t cat);

void flog1(const wchar_t *);

void flog1(const char *);

void flog1(const wcstring &s) { flog1(s.c_str()); }

void flog1(const std::string &s) { flog1(s.c_str()); }

template <typename T>
void flog1(std::enable_if<std::is_integral<T>::value, T> v) {
    flog1(to_string(v));
}

template <typename T>
void do_flog(const T &arg) {
    flog1(std::forward(arg));
}

template <typename T, typename... Ts>
void do_flog(const T &arg, const Ts &... rest) {
    flog1(arg);
    do_flog<Ts...>(rest...);
}
}  // namespace flog_details

#define FLOG(wht, ...)                                             \
    do {                                                           \
        if (flog_details::should_flog(fish_log_category_t::wht)) { \
            flog_details::fish_flog(__VA_ARGS__);                  \
        }                                                          \
    } while (0)

#endif
