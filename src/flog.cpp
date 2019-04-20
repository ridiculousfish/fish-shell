// fish logging
#include "config.h"

#include "flog.h"

#include "common.h"
#include "enum_set.h"
#include "wildcard.h"

#include <atomic>

using namespace flog_details;

template <>
struct enum_info_t<fish_log_category_t> {
    static constexpr auto count = fish_log_category_t::COUNT;
};

/// The thread-safe global set of fish-log categories.
static std::atomic<enum_set_t<fish_log_category_t>> flog_set{
    {fish_log_category_t::ohno, fish_log_category_t::debug}};

static inline const wchar_t *name_for_category(fish_log_category_t cat) {
    switch (cat) {
        case fish_log_category_t::ohno:
            return L"ohno";
        case fish_log_category_t::debug:
            return L"debug";
        case fish_log_category_t::COUNT:
            DIE("Invalid fish_log_category");
            return nullptr;
    }
}

/// Set (or clear, if \p sense is false) the bits in \p cats that match the wildcard \p change_wc.
static void apply_one_category(const wcstring &change_wc, enum_set_t<fish_log_category_t> *cats,
                               bool sense) {
    for (auto cat : enum_iter_t<fish_log_category_t>()) {
        if (wildcard_match(name_for_category(cat), change_wc)) {
            cats->set(cat, sense);
        }
    }
}

void set_flog_categories_by_pattern(const wcstring &str) {
    enum_set_t<fish_log_category_t> flogs{};
    for (const wcstring &s : split_string(str, L',')) {
        if (string_prefixes_string(s, L"-")) {
            apply_one_category(s.substr(1), &flogs, true);
        } else {
            apply_one_category(s, &flogs, false);
        }
    }
    flog_set.store(flogs, std::memory_order_relaxed);
}

bool should_flog(fish_log_category_t cat) {
    auto flogs = flog_set.load(std::memory_order_relaxed);
    return flogs.get(cat);
}

void flog1(const wchar_t *s) { std::fputws(s, stderr); }

void flog1(const char *s) { std::fputs(s, stderr); }
