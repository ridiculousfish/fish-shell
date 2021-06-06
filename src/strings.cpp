// Implementation of imstring.

#include "config.h"  // IWYU pragma: keep

#include "strings.h"

#include "common.h"

#define nssv_CONFIG_NO_EXCEPTIONS 1
#include "nonstd/string_view.hpp"

// static
inline imstring::shared_repr_t imstring::make_shared_repr(std::wstring &&str) {
    return shared_repr_t{repr_tag_t::shared, std::make_shared<std::wstring>(std::move(str))};
}

// static
imstring::shared_repr_t imstring::make_shared_repr(const wchar_t *ptr, size_t len) {
    return shared_repr_t{repr_tag_t::shared, std::make_shared<std::wstring>(ptr, len)};
}

void imstring::repr_t::destroy() {
    switch (tag()) {
        case repr_tag_t::literal:
            literal().~literal_repr_t();
            break;
        case repr_tag_t::unowned:
            unowned().~unowned_repr_t();
            break;
        case repr_tag_t::shared:
            shared().~shared_repr_t();
            break;
    }
}

imstring::repr_t::~repr_t() { this->destroy(); }

imstring::imstring(const imstring &rhs) { set_or_copy_from(rhs); }

imstring::imstring(wcstring &&rhs) : repr_(make_shared_repr(std::move(rhs))) {}

imstring &imstring::operator=(const imstring &rhs) {
    if (this != &rhs) {
        set_or_copy_from(rhs);
    }
    return *this;
}

void imstring::set_or_copy_from(const imstring &rhs) {
    switch (rhs.repr_.tag()) {
        case repr_tag_t::literal:
            this->repr_.set(rhs.repr_.literal());
            break;
        case repr_tag_t::unowned:
            this->repr_.set(make_shared_repr(rhs.data(), rhs.size()));
            break;
        case repr_tag_t::shared:
            this->repr_.set(rhs.repr_.shared());
            break;
    }
}

imstring imstring::substr(size_t pos, size_t count) const {
    // TODO: this can be made more efficient in some cases.
    assert(pos <= size() && "Position out of bounds");
    size_t eff_count = std::min(count, size() - pos);
    imstring result;
    result.repr_.set(make_shared_repr(this->data() + pos, eff_count));
    return result;
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
