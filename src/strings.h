#ifndef FISH_STRINGS_H
#define FISH_STRINGS_H

#include "config.h"  // IWYU pragma: keep

#include <cassert>
#include <iterator>
#include <memory>
#include <string>
#include <type_traits>
#include <vector>

#include "nonstd/string_view.hpp"

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
/// imstring wraps wstring_view, exposing functions like find_first_of, etc.
class imstring : public nonstd::wstring_view {
   public:
    static constexpr size_type npos = wcstring::npos;

    /// Default initialization is empty.
    constexpr imstring() : nonstd::wstring_view(L"", 0), storage_{} {}

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
        : nonstd::wstring_view(s.data(), s.size()), storage_{}, needs_copy_(true) {}

    /// Construct, referencing a string literal. This will copy the contents if the imstring is
    /// copied.
    template <class T, bool_if_same<T, const wchar_t *> = true>
    /* implicit */ constexpr imstring(const T &s REF_BY_RET)
        : nonstd::wstring_view(s, wcslen(s)), storage_{}, needs_copy_(true) {}

    /// Construct from a nul-terminated string literal with a size known at compile time.
    /// This allows zero-copy construction from literals: imstring s = L"foo";
    /// N-1 for nul terminator.
    template <size_t N>
    /* implicit */ constexpr imstring(const wchar_t (&str REF_BY_RET)[N])
        : nonstd::wstring_view(str, N - 1) {}

    /// Construct from a non-const array.
    /// This overload ensures that local non-const char arrays result in non-owning strings.
    /// This is useful for local buffers, e.g. with swprintf().
    /// Note here the string must be nul-terminated; we don't assume the string fills the entire
    /// array.
    template <size_t N>
    /* implicit */ imstring(wchar_t (&str REF_BY_RET)[N])
        : imstring(static_cast<const wchar_t *>(str)) {}

    /// Set an imstring to empty.
    void clear();

    /// \return a nul-terminated C string.
    const wchar_t *c_str() const REF_BY_RET { return data(); }

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
    value_type at(size_type idx) const {
        assert(idx < size() && "Index out of bounds");
        return data()[idx];
    }

    /// \return the character at \p idx, which must be <= length.
    /// If idx == length, this returns the nul terminator.
    value_type operator[](size_type idx) const {
        // <= to allow accessing nul-terminator.
        assert(idx <= size() && "Index out of bounds");
        return data()[idx];
    }

    bool operator==(const imstring &rhs) const {
        return *this == static_cast<const nonstd::wstring_view &>(rhs);
    }
    bool operator!=(const imstring &rhs) const {
        return *this != static_cast<const nonstd::wstring_view &>(rhs);
    }
    bool operator<(const imstring &rhs) const {
        return *this < static_cast<const nonstd::wstring_view &>(rhs);
    }
    bool operator<=(const imstring &rhs) const {
        return *this <= static_cast<const nonstd::wstring_view &>(rhs);
    }
    bool operator>(const imstring &rhs) const {
        return *this > static_cast<const nonstd::wstring_view &>(rhs);
    }
    bool operator>=(const imstring &rhs) const {
        return *this >= static_cast<const nonstd::wstring_view &>(rhs);
    }

    /// \return a substring from an offset. It must extend to the end of the string to preserve
    /// nul-terminator. This is often efficient: it just advances the pointer. We must have pos <=
    /// size().
    imstring substr(size_t pos) const;

    /// \return a substring with an offset and count. This always allocates a new string.
    wcstring substr_wcstring(size_t pos, size_t count = npos) const;

    /// Delete the inherited remove_suffix function, which would leave us non nul-terminated.
    void remove_suffix(size_t n) = delete;

    /// \return a wcstring, copying the contents.
    wcstring to_wcstring() const { return wcstring(data(), size()); }

    /// \return a hash value. Cover over wstring_view.
    size_t hash() const { return std::hash<nonstd::wstring_view>{}(*this); }

    /// A description of our storage's ownership. This is exposed for testing.
    enum backing_type_t {
        literal,  // our contents are a string literal, never need to be copied
        unowned,  // our contents are a reference to a wcstring, needs copying
        owned,    // our contents are backed by our shared_ptr
    };
    backing_type_t get_backing_type() const {
        assert(!(needs_copy_ && storage_) && "Should not have storage if we need copying");
        if (storage_) return owned;
        if (needs_copy_) return unowned;
        return literal;
    }

    ~imstring();

   private:
    // Helper constructor from a shared ptr.
    explicit imstring(std::shared_ptr<const wcstring> str);

    // Helper constructor from a shared ptr, pointer, and length.
    // This is useful for when the pointer points into the string, rather than at its first char.
    imstring(const wchar_t *contents REF_BY_RET, size_t len, std::shared_ptr<const wcstring> ptr);

    // Helper constructor for string literals.
    constexpr imstring(const wchar_t *str REF_BY_RET, size_t len, bool needs_copy)
        : nonstd::wstring_view(str, len), storage_{}, needs_copy_(needs_copy) {}

    // Set from another imstring, perhaps copying it.
    void set_or_copy_from(const imstring &rhs);

    /// Set our pointer and length.
    inline void set_string_view(const wchar_t *ptr, size_t len);

    // If we are backed by an std::wstring, then that string.
    // If we are backed by a string literal, this is null.
    std::shared_ptr<const wcstring> storage_{};

    // If set, then our pointer points at something that is likely to be deallocated (i.e. not a
    // literal and not our storage), and so our copy constructor must copy it.
    bool needs_copy_{false};

    friend imstring operator"" _im(const wchar_t *, size_t);
};

using imstring_list_t = std::vector<imstring>;

inline imstring operator"" _im(const wchar_t *str REF_BY_RET, size_t len) {
    return imstring(str, len, false /* needs_copy */);
}

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
