#ifndef FISH_STRINGS_H
#define FISH_STRINGS_H

#include "config.h"  // IWYU pragma: keep

#include <cassert>
#include <iterator>
#include <memory>
#include <string>
#include <type_traits>
#include <vector>

using wcstring = std::wstring;
using wcstring_list_t = std::vector<wcstring>;

/// A global, empty string. This is useful for functions which wish to return a reference to an
/// empty string.
extern const wcstring g_empty_string;

/// An attribute to apply to a function argument, which indicates that the return value of the
/// function references the argument. Clang will warn about unsafe uses here.
#if defined(__clang__)
#define REF_BY_RET [[clang::lifetimebound]]
#else
#define REF_BY_RET
#endif

/// An immutable string type, which wraps either a wide string literal or a std::wstring.
/// This is immutable in the sense that the string contents can never change (but this may be
/// assigned a new string).
/// imstring is always nul-terminated. You may use c_str() to efficiently access it as a
/// nul-terminated C string. Likewise operator[] will return the nul-terminator.
class imstring {
   public:
    static constexpr size_t npos = wcstring::npos;

    using size_type = size_t;
    using value_type = wchar_t;

    // Our iterators are const since we are immutable.
    using iterator = const wchar_t *;
    using const_iterator = const wchar_t *;
    using reverse_iterator = std::reverse_iterator<iterator>;
    using const_reverse_iterator = std::reverse_iterator<const_iterator>;

    /// Default initialization is empty.
    constexpr imstring() = default;

    /// Construct by taking ownership of a wcstring.
    /* implicit */ imstring(wcstring &&s);

    /// Helper template which is bool if T1 and T2 are the same, SFINAE'd away otherwise.
    /// This is a template so as to prevent type conversions. For example, if we had a constructor
    /// that just took `const wcstring &`, then it might be invoked by a const wchar_t * parameter.
    template <typename T1, typename T2>
    using bool_if_same = typename std::enable_if<std::is_same<T1, T2>::value, bool>::type;

    /// Construct, referencing a wcstring. This will copy the contents when the imstring is copied.
    template <class T, bool_if_same<T, wcstring> = true>
    constexpr /* implicit */ imstring(const T &s REF_BY_RET)
        : repr_(unowned_repr_t{repr_tag_t::unowned, s.data(), s.size()}) {}

    /// Construct, referencing a string literal. This will copy the contents if the imstring is
    /// copied.
    template <class T, bool_if_same<T, const wchar_t *> = true>
    /* implicit */ constexpr imstring(const T &s REF_BY_RET)
        : repr_(unowned_repr_t{repr_tag_t::unowned, s, wcslen(s)}) {}

    /// Construct from a nul-terminated string literal with a size known at compile time.
    /// This allows zero-copy construction from literals: imstring s = L"foo";
    /// N-1 for nul terminator.
    template <size_t N>
    /* implicit */ constexpr imstring(const wchar_t (&str REF_BY_RET)[N])
        : repr_(literal_repr_t{repr_tag_t::literal, str, N - 1}) {}

    /// Construct from a non-const array.
    /// This overload ensures that local non-const char arrays result in non-owning strings.
    /// This is useful for local buffers, e.g. with swprintf().
    /// Note here the string must be nul-terminated; we don't assume the string fills the entire
    /// array.
    template <size_t N>
    /* implicit */ imstring(wchar_t (&str REF_BY_RET)[N])
        : imstring(static_cast<const wchar_t *>(str)) {}

    /// Set an imstring to empty.
    void clear() { this->repr_.clear(); }

    /// \return if we are empty.
    bool empty() const { return size() == 0; }

    /// \return the number of chars in the string.
    size_t size() const { return repr_.length(); }
    size_t length() const { return repr_.length(); }

    /// \return a nul-terminated C string.
    const wchar_t *data() const REF_BY_RET { return repr_.ptr(); }
    const wchar_t *c_str() const REF_BY_RET { return repr_.ptr(); }

    /// Copying is typically cheap, but may trigger lazily copying the underlying buffer.
    imstring(const imstring &);
    imstring &operator=(const imstring &);

    /// Moving is implemented in terms of copying.
    imstring(imstring &&rhs) : imstring(static_cast<const imstring &>(rhs)) {}

    imstring &operator=(imstring &&rhs) {
        *this = static_cast<const imstring &>(rhs);
        return *this;
    }

    /// \return the character at \p idx, which must be < length.
    wchar_t at(size_t idx) const {
        assert(idx < size() && "Index out of bounds");
        return data()[idx];
    }

    /// \return the character at \p idx, which must be <= length.
    /// If idx == length, this returns the nul terminator.
    wchar_t operator[](size_t idx) const {
        // <= to allow accessing nul-terminator.
        assert(idx <= size() && "Index out of bounds");
        return data()[idx];
    }

    /// \return the first (last) character. Note we do not return a reference.
    wchar_t front() const { return at(0); }
    wchar_t back() const { return at(size() - 1); }

    bool operator==(const imstring &rhs) const { return rhs.size() == size() && compare(rhs) == 0; }
    bool operator==(const wchar_t *rhs) const { return compare(rhs) == 0; }

    bool operator!=(const imstring &rhs) const { return !(*this == rhs); }
    bool operator!=(const wchar_t *rhs) const { return !(*this == rhs); }

    bool operator<(const imstring &rhs) const { return compare(rhs) < 0; }
    bool operator<(const wchar_t *rhs) const { return compare(rhs) < 0; }

    bool operator>(const imstring &rhs) const { return compare(rhs) > 0; }
    bool operator>(const wchar_t *rhs) const { return compare(rhs) > 0; }

    bool operator<=(const imstring &rhs) const { return compare(rhs) <= 0; }
    bool operator<=(const wchar_t *rhs) const { return compare(rhs) <= 0; }

    bool operator>=(const imstring &rhs) const { return compare(rhs) >= 0; }
    bool operator>=(const wchar_t *rhs) const { return compare(rhs) >= 0; }

    /// \return -1, 0, or 1 if this is less than, equal to, or greater than \p str.
    /// This simply compares wchar_ts directly - no fancy collation.
    int compare(const wchar_t *str, size_t len) const;
    int compare(const imstring &str) const { return compare(str.data(), str.size()); }
    int compare(const wcstring &str) const { return compare(str.data(), str.size()); }

    /// Variant of compare() that accepts a nul-terminated stirng.
    int compare(const wchar_t *str) const;

    /// Compare a range [pos1, pos1+count1) of 'this' to the given string.
    /// The length may extend beyond the string; in that case it is truncated, following the STL.
    int compare(size_t pos1, size_t count1, const wchar_t *str, size_t len) const;
    int compare(size_t pos1, size_t count1, const imstring &str) const {
        return compare(pos1, count1, str.data(), str.size());
    }

    /// \return a substring from an offset.
    imstring substr(size_t pos, size_t count = npos) const;

    /// \return a substring with an offset and count. This always allocates a new string.
    wcstring substr_wcstring(size_t pos, size_t count = npos) const;

    /// \return a wcstring, copying the contents.
    wcstring to_wcstring() const { return wcstring(data(), size()); }

    /// \return a hash value.
    size_t hash() const {
        // We do NOT cover over wstring_view, it constructs a std::string (!!).
        // This is a basic sdbm hash.
        size_t hash = 0;
        for (wchar_t c : *this) {
            hash = c + (hash << 6) + (hash << 16) - hash;
        }
        return hash;
    }

    // Note we are immutable, so all iterators are const.
    const_iterator begin() const { return data(); }
    const_iterator end() const { return data() + size(); }

    const_iterator cbegin() const { return data(); }
    const_iterator cend() const { return data() + size(); }

    const_reverse_iterator rbegin() const { return reverse_iterator(end()); }
    const_reverse_iterator rend() const { return reverse_iterator(begin()); }

    const_reverse_iterator crbegin() const { return reverse_iterator(cend()); }
    const_reverse_iterator crend() const { return reverse_iterator(cbegin()); }

    // Covers over string_view methods.
    size_t find(const imstring &v, size_t pos = 0) const;
    size_t find(wchar_t ch, size_t pos = 0) const;
    size_t find(const wchar_t *s, size_t pos, size_t count) const;
    size_t find(const wchar_t *s, size_t pos = 0) const;

    /// \return the index of the first character in 'this' contained in \p s, or npos.
    size_t find_first_of(const imstring &s, size_t pos = 0) const;
    size_t find_first_of(wchar_t c, size_t pos = 0) const;
    size_t find_first_of(const wchar_t *s, size_t pos, size_t count) const;
    size_t find_first_of(const wchar_t *s, size_t pos = 0) const;

    /// \return the index of the first character in 'this' not contained in \p s, or npos.
    size_t find_first_not_of(const imstring &s, size_t pos = 0) const;
    size_t find_first_not_of(wchar_t c, size_t pos = 0) const;
    size_t find_first_not_of(const wchar_t *s, size_t pos, size_t count) const;
    size_t find_first_not_of(const wchar_t *s, size_t pos = 0) const;

    /// \return the index of the last character in 'this' contained in \p str, or npos.
    size_t find_last_of(const imstring &s, size_t pos = npos) const;
    size_t find_last_of(wchar_t c, size_t pos = npos) const;
    size_t find_last_of(const wchar_t *s, size_t pos, size_t count) const;
    size_t find_last_of(const wchar_t *s, size_t pos = npos) const;

    /// \return the index of the last character in 'this' not contained in \p str, or npos.
    size_t find_last_not_of(const imstring &s, size_t pos = npos) const noexcept;
    size_t find_last_not_of(wchar_t c, size_t pos = npos) const noexcept;
    size_t find_last_not_of(const wchar_t *s, size_t pos, size_t count) const;
    size_t find_last_not_of(const wchar_t *s, size_t pos = npos) const;

    /// \return the index of the last occurrence of \p v, where \p pos is the last valid
    /// return.
    size_t rfind(const imstring &v, size_t pos = npos) const;
    size_t rfind(wchar_t c, size_t pos = npos) const;
    size_t rfind(const wchar_t *s, size_t pos, size_t count) const;
    size_t rfind(const wchar_t *s, size_t pos = npos) const;

    /// Our possible representations.
    enum class repr_tag_t : uint8_t {
        literal,  // we are backed by a string literal
        inlined,  // we are backed by inlined storage
        unowned,  // we are backed by a transient string; copy it when we are copied or moved
        shared,   // we are backed by a shared_ptr<const wcstring>
    };
    repr_tag_t get_backing_type() const { return repr_.tag(); }

    ~imstring() = default;

   private:
    // Set this equal to another string, perhaps copying it.
    void set_or_copy_from(const imstring &rhs);

    struct literal_repr_t {
        repr_tag_t tag;
        const wchar_t *ptr;
        size_t len;
    };
    struct inlined_repr_t {
        // we may store up to 4 bytes inline, plus nul term.
        // The intent here is to be 24 bytes long.
        static constexpr size_t kInlineCharCount = 4;
        repr_tag_t tag;
        uint8_t len;
        wchar_t storage[kInlineCharCount + 1];
    };
    struct unowned_repr_t {
        repr_tag_t tag;
        const wchar_t *ptr;
        size_t len;
    };
    struct shared_repr_t {
        repr_tag_t tag;
        std::shared_ptr<std::wstring> ptr;
    };

    // Helper function for creating an empty literal representation.
    static constexpr inline literal_repr_t empty_literal() {
        return literal_repr_t{repr_tag_t::literal, L"", 0};
    }

    /// Helper function for making a shared and owned representation.
    static inline shared_repr_t make_shared_repr(const wchar_t *ptr, size_t len);
    static inline shared_repr_t make_shared_repr(std::wstring &&str);
    static inline inlined_repr_t make_inlined_repr(const wchar_t *ptr, size_t len);

    union repr_t {
       public:
        // Construct as empty.
        constexpr repr_t() : literal_(empty_literal()) {}

        // Construct from our different representations.
        explicit constexpr repr_t(literal_repr_t v) : literal_(v) {}
        explicit constexpr repr_t(unowned_repr_t v) : unowned_(v) {}
        explicit constexpr repr_t(const inlined_repr_t &v) : inlined_(v) {}
        explicit repr_t(shared_repr_t &&v) : shared_(std::move(v)) {}

        // Assert that our tag is what we expect.
        void check_tag(repr_tag_t tag) const { assert(tag == tag_.tag && "Wrong tag"); }

        // \return our tag.
        repr_tag_t tag() const { return tag_.tag; }

        // Clear this repr, making us literal empty string.
        void clear() { set(imstring::empty_literal()); }

        // Access our underlying pointer.
        const wchar_t *ptr() const {
            switch (tag()) {
                case repr_tag_t::literal:
                    return literal().ptr;
                case repr_tag_t::inlined:
                    return inlined().storage;
                case repr_tag_t::unowned:
                    return unowned().ptr;
                case repr_tag_t::shared:
                    return shared().ptr->data();
            }
        }

        // Access our underlying length.
        size_t length() const {
            switch (tag()) {
                case repr_tag_t::literal:
                    return literal().len;
                case repr_tag_t::inlined:
                    return inlined().len;
                case repr_tag_t::unowned:
                    return unowned().len;
                case repr_tag_t::shared:
                    return shared().ptr->length();
            }
        }

        // Access our three representations, asserting on the tag.
        literal_repr_t &literal() {
            check_tag(repr_tag_t::literal);
            return literal_;
        }

        const literal_repr_t &literal() const {
            check_tag(repr_tag_t::literal);
            return literal_;
        }

        unowned_repr_t &unowned() {
            check_tag(repr_tag_t::unowned);
            return unowned_;
        }

        const unowned_repr_t &unowned() const {
            check_tag(repr_tag_t::unowned);
            return unowned_;
        }

        inlined_repr_t &inlined() {
            check_tag(repr_tag_t::inlined);
            return inlined_;
        }

        const inlined_repr_t &inlined() const {
            check_tag(repr_tag_t::inlined);
            return inlined_;
        }

        shared_repr_t &shared() {
            check_tag(repr_tag_t::shared);
            return shared_;
        }
        const shared_repr_t &shared() const {
            check_tag(repr_tag_t::shared);
            return shared_;
        }

        // Set from our three representations.
        void set(literal_repr_t v) {
            destroy();
            new (&literal_) literal_repr_t(v);
        }

        void set(unowned_repr_t v) {
            destroy();
            new (&unowned_) unowned_repr_t(v);
        }

        void set(shared_repr_t v) {
            destroy();
            new (&shared_) shared_repr_t(std::move(v));
        }

        void set(const inlined_repr_t &v) {
            destroy();
            new (&inlined_) inlined_repr_t(v);
        }

        // We have our own move and copy logic.
        repr_t(const repr_t &);
        repr_t &operator=(const repr_t &);
        repr_t(repr_t &&);
        repr_t &operator=(repr_t &&);

        ~repr_t();

       private:
        // Invoke the proper destructor of our field.
        // After invoking this, you must use placement new on a field to populate it.
        void destroy();

        literal_repr_t literal_;
        unowned_repr_t unowned_;
        inlined_repr_t inlined_;
        shared_repr_t shared_;
        struct {
            repr_tag_t tag;
        } tag_;
    };
    repr_t repr_;

    // Helper constructors.
    enum class no_copy_t { nocopy };
    constexpr imstring(const wchar_t *str REF_BY_RET, size_t len, no_copy_t)
        : repr_(literal_repr_t{repr_tag_t::literal, str, len}) {}

    friend imstring operator"" _im(const wchar_t *, size_t);
};

using imstring_list_t = std::vector<imstring>;

inline imstring operator"" _im(const wchar_t *str REF_BY_RET, size_t len) {
    return imstring(str, len, imstring::no_copy_t::nocopy);
}

/// Allow comparing against const wchar_t * and const wcstring &.
inline bool operator==(const wchar_t *lhs, const imstring &rhs) { return rhs == lhs; }
inline bool operator!=(const wchar_t *lhs, const imstring &rhs) { return rhs != lhs; }
inline bool operator<(const wchar_t *lhs, const imstring &rhs) { return rhs > lhs; }
inline bool operator>(const wchar_t *lhs, const imstring &rhs) { return rhs < lhs; }
inline bool operator<=(const wchar_t *lhs, const imstring &rhs) { return rhs >= lhs; }
inline bool operator>=(const wchar_t *lhs, const imstring &rhs) { return rhs <= lhs; }

inline bool operator==(const wcstring &lhs, const imstring &rhs) { return rhs == lhs; }
inline bool operator!=(const wcstring &lhs, const imstring &rhs) { return rhs != lhs; }
inline bool operator<(const wcstring &lhs, const imstring &rhs) { return rhs > lhs; }
inline bool operator>(const wcstring &lhs, const imstring &rhs) { return rhs < lhs; }
inline bool operator<=(const wcstring &lhs, const imstring &rhs) { return rhs >= lhs; }
inline bool operator>=(const wcstring &lhs, const imstring &rhs) { return rhs <= lhs; }

/// Allow appending with wcstring.
inline wcstring operator+(const imstring &lhs, const wcstring &rhs) {
    wcstring res = lhs.to_wcstring();
    res.append(rhs);
    return res;
}

inline wcstring operator+(wcstring lhs, const imstring &rhs) {
    wcstring res = std::move(lhs);
    res.append(rhs.data(), rhs.size());
    return res;
}

inline wcstring &operator+=(wcstring &lhs, const imstring &rhs) {
    lhs.append(rhs.data(), rhs.size());
    return lhs;
}

namespace std {
template <>
struct hash<imstring> {
    size_t operator()(const imstring &s) const { return s.hash(); }
};
}  // namespace std

#endif
