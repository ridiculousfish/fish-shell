// Implementation of imstring.

#include "config.h"  // IWYU pragma: keep

#include "strings.h"

#include "common.h"

imstring::~imstring() = default;

imstring::imstring(const imstring &rhs) : nonstd::wstring_view() { set_or_copy_from(rhs); }

imstring &imstring::operator=(const imstring &rhs) {
    if (this != &rhs) {
        set_or_copy_from(rhs);
    }
    return *this;
}

inline void imstring::set_string_view(const wchar_t *ptr, size_t len) {
    *static_cast<nonstd::wstring_view *>(this) = nonstd::wstring_view(ptr, len);
}

void imstring::set_or_copy_from(const imstring &rhs) {
    if (rhs.empty()) {
        this->clear();
    } else if (rhs.needs_copy_) {
        storage_ = std::make_shared<const wcstring>(rhs.data(), rhs.size());
        set_string_view(storage_->data(), storage_->size());
    } else {
        storage_ = rhs.storage_;
        set_string_view(rhs.data(), rhs.size());
    }
    needs_copy_ = false;
}

void imstring::clear() {
    storage_.reset();
    set_string_view(L"", 0);
    needs_copy_ = false;
}

imstring imstring::substr(size_t pos) const {
    assert(pos <= size() && "Position out of bounds");
    imstring res = *this;  // trigger copying
    res.set_string_view(res.data() + pos, res.size() - pos);
    return res;
}

wcstring imstring::substr_wcstring(size_t pos, size_t count) const {
    assert(pos <= size() && "Position out of bounds");
    size_t eff_count = std::min(count, size() - pos);
    return wcstring(data() + pos, eff_count);
}

imstring::imstring(const wchar_t *contents, size_t len, std::shared_ptr<const wcstring> ptr)
    : nonstd::wstring_view(contents, len), storage_(std::move(ptr)) {}

imstring::imstring(std::shared_ptr<const wcstring> str)
    : nonstd::wstring_view(str->data(), str->size()), storage_(std::move(str)) {}

imstring::imstring(wcstring &&s) : imstring(std::make_shared<const wcstring>(std::move(s))) {}
