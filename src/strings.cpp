// Implementation of imstring.

#include "config.h"  // IWYU pragma: keep

#include "strings.h"

#include "common.h"
#include "flog.h"

#define nssv_CONFIG_NO_EXCEPTIONS 1
#include "nonstd/string_view.hpp"

// static
void imstring::static_asserts() {
    // Ensure that our representations agree on tag locations.
    static_assert(offsetof(data_storage_t, data) == offsetof(inlined_repr_t, data),
                  "data offsets do not align");
    static_assert(offsetof(data_storage_t, data) == offsetof(literal_repr_t, data),
                  "data offsets do not align");
    static_assert(offsetof(data_storage_t, data) == offsetof(unowned_repr_t, data),
                  "data offsets do not align");
    static_assert(offsetof(data_storage_t, data) == offsetof(sharedarr_repr_t, data),
                  "data offsets do not align");
}

// static
imstring::sharedarr_t *imstring::sharedarr_t::create(const wchar_t *ptr, size_t len) {
    // Note sharedarr_t::chars is an array of size 1; we need this 1 for our nul terminator.
    size_t mem_size = sizeof(sharedarr_t) + len * sizeof(wchar_t);
    sharedarr_t *result = static_cast<sharedarr_t *>(malloc(mem_size));
    if (!result) {
        FLOGF(error, "malloc(%lu) failed", (unsigned long)mem_size);
        abort();
    }
    result->rc = 1;
    std::uninitialized_copy_n(ptr, len, result->chars);
    result->chars[len] = L'\0';
    return result;
}

void imstring::sharedarr_t::increment_rc() {
    uint32_t newrc = 1 + rc.fetch_add(1, std::memory_order_relaxed);
    assert(newrc > 0 && "Refcount overflow");
    (void)newrc;
}

void imstring::sharedarr_t::decrement_rc() {
    uint32_t oldrc = rc.fetch_sub(1, std::memory_order_relaxed);
    assert(oldrc > 0 && "Refcount underflow");
    if (oldrc == 1) free(this);
}

imstring::sharedarr_repr_t::sharedarr_repr_t(const wchar_t *ptr, size_t len)
    : pointer_base_repr_t<sharedarr_t, repr_tag_t::sharedarr>(sharedarr_t::create(ptr, len), len) {}

imstring::sharedarr_repr_t::sharedarr_repr_t(const imstring::sharedarr_repr_t &rhs)
    : pointer_base_repr_t<sharedarr_t, repr_tag_t::sharedarr>(rhs.ptr, rhs.len) {
    this->ptr->increment_rc();
}

imstring::sharedarr_repr_t::~sharedarr_repr_t() { this->ptr->decrement_rc(); }

void imstring::repr_t::destroy() {
    switch (tag()) {
        case repr_tag_t::literal:
            literal().~literal_repr_t();
            break;
        case repr_tag_t::unowned:
            unowned().~unowned_repr_t();
            break;
        case repr_tag_t::inlined:
            inlined().~inlined_repr_t();
            break;
        case repr_tag_t::sharedarr:
            sharedarr().~sharedarr_repr_t();
            break;
    }
}

imstring::repr_t::~repr_t() { this->destroy(); }

imstring::imstring(const imstring &rhs) { set_or_copy_from(rhs); }

imstring::imstring(const wchar_t *str, size_t len) {
    if (len <= kMaxInlineCharCount) {
        this->repr_.set(inlined_repr_t(str, len));
    } else {
        this->repr_.set(sharedarr_repr_t(str, len));
    }
}

imstring &imstring::operator=(const imstring &rhs) {
    if (this != &rhs) set_or_copy_from(rhs);
    return *this;
}

void imstring::set_or_copy_from(const imstring &rhs) {
    switch (rhs.repr_.tag()) {
        case repr_tag_t::literal:
            this->repr_.set(rhs.repr_.literal());
            break;
        case repr_tag_t::unowned:
            if (rhs.size() <= kMaxInlineCharCount) {
                this->repr_.set(inlined_repr_t(rhs.c_str(), rhs.size()));
            } else {
                this->repr_.set(sharedarr_repr_t(rhs.c_str(), rhs.size()));
            }
            break;
        case repr_tag_t::inlined:
            this->repr_.set(rhs.repr_.inlined());
            break;
        case repr_tag_t::sharedarr:
            this->repr_.set(rhs.repr_.sharedarr());
            break;
    }
}

imstring imstring::substr(size_t pos, size_t count) const {
    size_t len = this->size();
    assert(pos <= len && "Position out of bounds");
    return imstring(this->data() + pos, std::min(count, len - pos));
}

wcstring imstring::substr_wcstring(size_t pos, size_t count) const {
    assert(pos <= size() && "Position out of bounds");
    size_t eff_count = std::min(count, size() - pos);
    return wcstring(data() + pos, eff_count);
}

// string_view cover methods.
static inline nonstd::wstring_view view(const imstring *v) {
    return nonstd::wstring_view(v->data(), v->size());
}

static inline nonstd::wstring_view view(const imstring &v) {
    return nonstd::wstring_view(v.data(), v.size());
}

int imstring::compare(size_t pos1, size_t count1, const wchar_t *str, size_t len) const {
    return view(this).compare(pos1, count1, nonstd::wstring_view(str, len));
}

int imstring::compare(const wchar_t *str, size_t len) const {
    return view(this).compare(nonstd::wstring_view(str, len));
}

int imstring::compare(const wchar_t *str) const { return view(this).compare(str); }

size_t imstring::find(const imstring &v, size_t pos) const { return view(this).find(view(v), pos); }

size_t imstring::find(wchar_t ch, size_t pos) const { return view(this).find(ch, pos); }

size_t imstring::find(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).find(s, pos, count);
}

size_t imstring::find(const wchar_t *s, size_t pos) const { return view(this).find(s, pos); }

size_t imstring::find_first_of(const imstring &s, size_t pos) const {
    return view(this).find_first_of(view(s), pos);
}
size_t imstring::find_first_of(wchar_t c, size_t pos) const {
    return view(this).find_first_of(c, pos);
}

size_t imstring::find_first_of(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).find_first_of(s, pos, count);
}
size_t imstring::find_first_of(const wchar_t *s, size_t pos) const {
    return view(this).find_first_of(s, pos);
}

size_t imstring::find_first_not_of(const imstring &s, size_t pos) const {
    return view(this).find_first_not_of(view(s), pos);
}
size_t imstring::find_first_not_of(wchar_t c, size_t pos) const {
    return view(this).find_first_not_of(c, pos);
}

size_t imstring::find_first_not_of(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).find_first_not_of(s, pos, count);
}
size_t imstring::find_first_not_of(const wchar_t *s, size_t pos) const {
    return view(this).find_first_not_of(s, pos);
}

size_t imstring::find_last_of(const imstring &s, size_t pos) const {
    return view(this).find_last_of(view(s), pos);
}
size_t imstring::find_last_of(wchar_t c, size_t pos) const {
    return view(this).find_last_of(c, pos);
}

size_t imstring::find_last_of(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).find_last_of(s, pos, count);
}
size_t imstring::find_last_of(const wchar_t *s, size_t pos) const {
    return view(this).find_last_of(s, pos);
}

size_t imstring::find_last_not_of(const imstring &s, size_t pos) const {
    return view(this).find_last_not_of(view(s), pos);
}
size_t imstring::find_last_not_of(wchar_t c, size_t pos) const {
    return view(this).find_last_not_of(c, pos);
}

size_t imstring::find_last_not_of(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).find_last_not_of(s, pos, count);
}
size_t imstring::find_last_not_of(const wchar_t *s, size_t pos) const {
    return view(this).find_last_not_of(s, pos);
}

size_t imstring::rfind(const imstring &v, size_t pos) const {
    return view(this).rfind(view(v), pos);
}
size_t imstring::rfind(wchar_t c, size_t pos) const { return view(this).rfind(c, pos); }
size_t imstring::rfind(const wchar_t *s, size_t pos, size_t count) const {
    return view(this).rfind(s, pos, count);
}
size_t imstring::rfind(const wchar_t *s, size_t pos) const { return view(this).rfind(s, pos); }
