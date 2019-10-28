#ifndef FISH_WCSTRING_H
#define FISH_WCSTRING_H

// Common string type.
class wcstring {
    using contents_t = std::wstring;
    using CharT = contents_t::value_type;

    std::shared_ptr<contents_t> s_;

    contents_t &s() {
        if (s_.use_count() > 1) {
            s_ = std::make_shared<contents_t>(*s_);
        }
        return *s_;
    }
    const contents_t &s() const { return *s_; }

    /// \return the singleton empty string.
    static std::shared_ptr<contents_t> get_shared_empty();

   public:
    using size_type = contents_t::size_type;
    using value_type = contents_t::value_type;
    using iterator = contents_t::const_iterator;
    using const_iterator = contents_t::const_iterator;
    using reverse_iterator = contents_t::reverse_iterator;
    using const_reverse_iterator = contents_t::const_reverse_iterator;

    static constexpr size_type npos = contents_t::npos;

    bool empty() const { return s().empty(); }

    bool operator==(const wcstring &rhs) const { return s() == rhs.s(); }
    bool operator==(const CharT *rhs) const { return s() == rhs; }

    bool operator!=(const wcstring &rhs) const { return s() != rhs.s(); }
    bool operator!=(const CharT *rhs) const { return s() != rhs; }

    bool operator<(const wcstring &rhs) const { return s() < rhs.s(); }
    bool operator<(const CharT *rhs) const { return s() < rhs; }

    bool operator>(const wcstring &rhs) const { return s() > rhs.s(); }
    bool operator>(const CharT *rhs) const { return s() > rhs; }

    bool operator<=(const wcstring &rhs) const { return s() <= rhs.s(); }
    bool operator<=(const CharT *rhs) const { return s() <= rhs; }

    bool operator>=(const wcstring &rhs) const { return s() >= rhs.s(); }
    bool operator>=(const CharT *rhs) const { return s() >= rhs; }

    value_type at(size_type idx) const { return s().at(idx); }
    value_type operator[](size_type idx) const { return s()[idx]; }

    size_type hash() const { return std::hash<contents_t>{}(s()); }

    iterator begin() { return s().begin(); }
    iterator end() { return s().end(); }

    const_iterator begin() const { return s().begin(); }
    const_iterator end() const { return s().end(); }

    const_iterator cbegin() const { return s().begin(); }
    const_iterator cend() const { return s().end(); }

    const_reverse_iterator rbegin() const { return s().rbegin(); }
    const_reverse_iterator rend() const { return s().rend(); }

    const_reverse_iterator crbegin() const { return s().crbegin(); }
    const_reverse_iterator crend() const { return s().crend(); }

    wcstring() : s_(get_shared_empty()) {}

    /* implicit */ wcstring(std::wstring &&s) : s_(std::make_shared<contents_t>(std::move(s))) {}
    /* implicit */ wcstring(const std::wstring &s) : s_(std::make_shared<contents_t>(s)) {}

    template <
        typename... Args,
        typename std::enable_if<std::is_constructible<contents_t, Args...>::value, int>::type = 0>
    wcstring(Args &&... args) : wcstring(std::wstring(std::forward<Args>(args)...)) {}

    wcstring(const wcstring &str, size_t pos, size_t count = npos)
        : wcstring(str.s(), pos, count) {}

    wcstring(std::initializer_list<CharT> ilist) : s_(std::make_shared<contents_t>(ilist)) {}

    wcstring(wcstring &&) = default;
    wcstring(const wcstring &) = default;
    wcstring &operator=(const wcstring &) = default;
    wcstring &operator=(wcstring &&) = default;

    wcstring &assign(size_type count, value_type c) {
        s().assign(count, c);
        return *this;
    }

    wcstring &assign(const wcstring &str) {
        s().assign(str.s());
        return *this;
    }

    wcstring &assign(const wcstring &str, size_type pos, size_type count) {
        s().assign(str.s(), pos, count);
        return *this;
    }

    wcstring &assign(wcstring &&str) {
        s_ = str.s_;
        return *this;
    }

    wcstring &assign(const CharT *str, size_type count) {
        s().assign(str, count);
        return *this;
    }

    wcstring &assign(const CharT *str) {
        s().assign(str);
        return *this;
    }

    template <class InputIt>
    wcstring &assign(InputIt first, InputIt last) {
        s().assign(first, last);
        return *this;
    }

    wcstring &assign(std::initializer_list<CharT> ilist) {
        s().assign(ilist);
        return *this;
    }

    wcstring &append(size_type count, CharT c) {
        s().append(count, c);
        return *this;
    }

    wcstring &append(const wcstring &str) {
        s().append(str.s());
        return *this;
    }

    wcstring &append(const wcstring &str, size_type pos, size_type count) {
        s().append(str.s(), pos, count);
        return *this;
    }

    wcstring &append(const CharT *str, size_type count) {
        s().append(str, count);
        return *this;
    }

    wcstring &append(const CharT *str) {
        s().append(str);
        return *this;
    }

    wcstring &append(std::initializer_list<CharT> ilist) {
        s().append(ilist);
        return *this;
    }

    template <class InputIt>
    wcstring &append(InputIt first, InputIt last) {
        s().append(first, last);
        return *this;
    }

    wcstring &replace(size_type pos, size_type count, const wcstring &str) {
        s().replace(pos, count, str.s());
        return *this;
    }

    wcstring &replace(const_iterator first, const_iterator last, const wcstring &str) {
        s().replace(first, last, str.s());
        return *this;
    }

    wcstring &replace(size_type pos, size_type count, const wcstring &str, size_type pos2,
                      size_type count2) {
        s().replace(pos, count, str.s(), pos2, count2);
        return *this;
    }

    template <class InputIt>
    wcstring &replace(const_iterator first, const_iterator last, InputIt first2, InputIt last2) {
        s().replace(first, last, first2, last2);
        return *this;
    }

    wcstring &replace(size_type pos, size_type count, const CharT *cstr, size_type count2) {
        s().replace(pos, count, cstr, count2);
        return *this;
    }

    wcstring &replace(const_iterator first, const_iterator last, const CharT *cstr,
                      size_type count2) {
        s().replace(first, last, cstr, count2);
        return *this;
    }

    wcstring &replace(size_type pos, size_type count, const CharT *cstr) {
        s().replace(pos, count, cstr);
        return *this;
    }

    wcstring &replace(const_iterator first, const_iterator last, const CharT *cstr) {
        s().replace(first, last, cstr);
        return *this;
    }

    wcstring &replace(size_type pos, size_type count, size_type count2, CharT ch) {
        s().replace(pos, count, count2, ch);
        return *this;
    }

    wcstring &replace(const_iterator first, const_iterator last, size_type count2, CharT ch) {
        s().replace(first, last, count2, ch);
        return *this;
    }

    wcstring &replace(const_iterator first, const_iterator last,
                      std::initializer_list<CharT> ilist) {
        s().replace(first, last, ilist);
        return *this;
    }

    wcstring &operator+=(const wcstring &rhs) {
        s() += rhs.s();
        return *this;
    }

    wcstring &operator+=(CharT c) {
        s() += c;
        return *this;
    }

    wcstring &operator+=(const CharT *str) {
        s() += str;
        return *this;
    }

    wcstring &operator+=(std::initializer_list<CharT> ilist) {
        s() += ilist;
        return *this;
    }

    value_type front() const { return s().front(); }

    value_type back() const { return s().back(); }

    void push_back(wchar_t c) { s().push_back(c); }

    void pop_back() { s().pop_back(); }

    size_type size() const { return s().size(); }
    size_type length() const { return s().length(); }

    void clear() { s_ = get_shared_empty(); }

    const wchar_t *c_str() const { return s().c_str(); }

    const wchar_t *data() const { return s().data(); }

    void reserve(size_type amt) { s().reserve(amt); }

    void resize(size_type count, CharT ch = CharT()) { s().resize(count, ch); }

    wcstring substr(size_type pos, size_type count = npos) const { return s().substr(pos, count); }

    size_type find(const wcstring &str, size_type pos = 0) const { return s().find(str.s(), pos); }

    size_type find(const CharT *str, size_type pos, size_type count) const {
        return s().find(str, pos, count);
    }

    size_type find(CharT ch, size_type pos = 0) const { return s().find(ch, pos); }

    size_type rfind(const wcstring &str, size_type pos = npos) const {
        return s().rfind(str.s(), pos);
    }

    size_type rfind(const CharT *str, size_type pos, size_type count) const {
        return s().rfind(str, pos, count);
    }

    size_type rfind(CharT ch, size_type pos = npos) const { return s().rfind(ch, pos); }

    size_type find_first_of(const wcstring &str, size_type pos = 0) const {
        return s().find_first_of(str.s(), pos);
    }

    size_type find_first_of(const CharT *str, size_type pos, size_type count) const {
        return s().find_first_of(str, pos, count);
    }

    size_type find_first_of(CharT ch, size_type pos = 0) const {
        return s().find_first_of(ch, pos);
    }

    size_type find_first_not_of(const wcstring &str, size_type pos = 0) const {
        return s().find_first_not_of(str.s(), pos);
    }

    size_type find_first_not_of(const CharT *str, size_type pos, size_type count) const {
        return s().find_first_not_of(str, pos, count);
    }

    size_type find_first_not_of(CharT ch, size_type pos = 0) const {
        return s().find_first_not_of(ch, pos);
    }

    size_type find_last_of(const wcstring &str, size_type pos = npos) const {
        return s().find_last_of(str.s(), pos);
    }

    size_type find_last_of(const CharT *str, size_type pos, size_type count) const {
        return s().find_last_of(str, pos, count);
    }

    size_type find_last_of(CharT ch, size_type pos = npos) const {
        return s().find_last_of(ch, pos);
    }

    size_type find_last_not_of(const wcstring &str, size_type pos = npos) const {
        return s().find_last_not_of(str.s(), pos);
    }

    size_type find_last_not_of(const CharT *str, size_type pos = npos) const {
        return s().find_last_not_of(str, pos);
    }

    size_type find_last_not_of(CharT ch, size_type pos = npos) const {
        return s().find_last_not_of(ch, pos);
    }

    wcstring &insert(size_type index, size_type count, CharT ch) {
        s().insert(index, count, ch);
        return *this;
    }

    wcstring &insert(size_type index, const CharT *str) {
        s().insert(index, str);
        return *this;
    }

    wcstring &insert(size_type index, const CharT *str, size_type count) {
        s().insert(index, str, count);
        return *this;
    }

    wcstring &insert(size_type index, const wcstring &str) {
        s().insert(index, str.s());
        return *this;
    }

    wcstring &insert(size_type index, const wcstring &str, size_type index_str, size_type count) {
        s().insert(index, str.s(), index_str, count);
        return *this;
    }

    iterator insert(iterator pos, CharT ch) { return s().insert(pos, ch); }

    iterator insert(const_iterator pos, size_type count, CharT ch) {
        return s().insert(pos, count, ch);
    }

    template <class InputIt>
    void insert(const_iterator pos, InputIt first, InputIt last) {
        s().insert(pos, first, last);
    }

    // we do not provide:
    //   iterator insert(const_iterator, std::initializer_list<CharT>)
    // because it appears gcc 5.4 does not support it.

    int compare(const wcstring &str) const { return s().compare(str.s()); }

    int compare(size_type pos1, size_type count1, const wcstring &str) const {
        return s().compare(pos1, count1, str.s());
    }

    int compare(size_type pos1, size_type count1, const wcstring &str, size_type pos2,
                size_type count2 = npos) const {
        return s().compare(pos1, count1, str.s(), pos2, count2);
    }

    int compare(const CharT *str) const { return s().compare(str); }

    int compare(size_type pos1, size_type count1, const CharT *str) const {
        return s().compare(pos1, count1, str);
    }

    wcstring &erase(size_type index = 0, size_type count = npos) {
        s().erase(index, count);
        return *this;
    }

    iterator erase(const_iterator position) { return s().erase(position); }

    iterator erase(const_iterator first, const_iterator last) { return s().erase(first, last); }

    /// Efficient support for mutating a string in place.
    /// Do not allow 'this' string to be copied while mutating, as the copy may see the mutations as
    /// well.
    std::wstring &mutate() { return s(); }

    std::wstring to_wstring() const { return s(); }
};

inline wcstring operator+(const wcstring &lhs, const wcstring &rhs) {
    wcstring ret{lhs};
    ret.append(rhs);
    return ret;
}

inline wcstring operator+(const wcstring &lhs, const wchar_t *rhs) {
    wcstring ret{lhs};
    ret.append(rhs);
    return ret;
}

inline wcstring operator+(const wcstring &lhs, wchar_t rhs) {
    wcstring ret{lhs};
    ret.append(1, rhs);
    return ret;
}

inline wcstring operator+(const wchar_t *lhs, const wcstring &rhs) {
    wcstring ret{lhs};
    ret.append(rhs);
    return ret;
}

inline wcstring operator+(wchar_t lhs, const wcstring &rhs) {
    wcstring ret(1, lhs);
    ret.append(rhs);
    return ret;
}

inline bool operator==(const wchar_t *lhs, const wcstring &rhs) { return rhs == lhs; }

inline bool operator!=(const wchar_t *lhs, const wcstring &rhs) { return rhs != lhs; }

inline bool operator<(const wchar_t *lhs, const wcstring &rhs) { return rhs > lhs; }

inline bool operator>(const wchar_t *lhs, const wcstring &rhs) { return rhs < lhs; }

inline bool operator<=(const wchar_t *lhs, const wcstring &rhs) { return rhs >= lhs; }

inline bool operator>=(const wchar_t *lhs, const wcstring &rhs) { return rhs <= lhs; }

namespace std {
template <>
struct hash<wcstring> {
    size_t operator()(const wcstring &s) const { return s.hash(); }
};
}  // namespace std

typedef std::vector<wcstring> wcstring_list_t;

#endif
