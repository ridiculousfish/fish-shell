#ifndef FISH_STRINGS_H
#define FISH_STRINGS_H

#include "config.h"  // IWYU pragma: keep

#include <atomic>
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

/// An immutable string type with polymorphic backing and a std::wstring compatible API.
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

    /// Helper template which is bool if T1 and T2 are the same, SFINAE'd away otherwise.
    /// This is a template so as to prevent type conversions. For example, if we had a constructor
    /// that just took `const wcstring &`, then it might be invoked by a const wchar_t * parameter.
    template <typename T1, typename T2>
    using bool_if_same = typename std::enable_if<std::is_same<T1, T2>::value, bool>::type;

    /// Construct, referencing a wcstring. This will copy the contents when the imstring is copied.
    template <class T, bool_if_same<T, wcstring> = true>
    constexpr /* implicit */ imstring(const T &s REF_BY_RET)
        : repr_(unowned_repr_t{s.data(), s.size()}) {}

    /// Helper to ensure we do not lazily reference a temporary wcstring.
    template <class T, bool_if_same<T, wcstring> = true>
    /* implicit */ imstring(T &&s) : imstring(s.data(), s.size()) {}

    /// Construct from a const C string. This will copy the contents if the imstring is copied.
    template <class T, bool_if_same<T, const wchar_t *> = true>
    /* implicit */ constexpr imstring(T s REF_BY_RET) : repr_(unowned_repr_t{s, wcslen(s)}) {}

    /// Construct from a string of the given length, which is eagerly copied to ensure
    /// nul-termination.
    imstring(const wchar_t *str, size_t len);

    /// Construct from a non-const C string. This also eagerly copies.
    /* implicit */ imstring(wchar_t *str) : imstring(str, wcslen(str)) {}

    /// Construct from a nul-terminated string literal with a size known at compile time.
    /// This allows zero-copy construction from literals: imstring s = L"foo";
    /// N-1 for nul terminator.
    template <size_t N>
    /* implicit */ constexpr imstring(const wchar_t (&str REF_BY_RET)[N])
        : repr_(literal_repr_t{str, N - 1}) {}

    /// Construct from a non-const array.
    /// This overload ensures that local non-const char arrays result in non-owning strings.
    /// This is useful for local buffers, e.g. with swprintf().
    /// Note here the string must be nul-terminated; we don't assume the string fills the entire
    /// array.
    template <size_t N>
    /* implicit */ imstring(wchar_t (&str REF_BY_RET)[N])
        : imstring(static_cast<const wchar_t *>(str)) {}

    /// Set an imstring to empty.
    void clear() { this->repr_.set(empty_literal()); }

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

    ~imstring() = default;

   private:
    /// Our representations. 'inlined' must be 0 as it may also serve as the nul terminator.
    enum class repr_tag_t : uint8_t {
        inlined = 0,  // we are backed by inlined storage
        literal,      // we are backed by a string literal
        unowned,      // we are backed by a transient string; copy it when we are copied or moved
        sharedarr,    // we are backed by a malloc'd sharedarr_t.
    };

    /// We store small strings inline.
    /// Here's the max number of chars we can store, not including the nul terminator.
    static constexpr size_t kMaxInlineCharCount = 5;

    /// A trailing word containing data about the string.
    /// Our string size is 24 bytes (assuming wchar_t is size 4).
    /// The data word occupies the LAST 4 bytes (2 bytes if wchar_t is 16 bit).
    /// The tag itself is the least-significant byte of the data word.
    /// If our string is inline, then the length is encoded in the second least-significant byte,
    /// as 5 - len. That is, a string of true length 5 will store a length of 0.
    /// The idea is that a string of length 5 has a 0 data word (0 tag and stored length 5-0),
    /// so the data word can play double-duty as the nul terminator.
    using data_word_t = wchar_t;

    // Helper to encode and decode tags from data word.
    static constexpr data_word_t encode_tag(repr_tag_t v) { return static_cast<data_word_t>(v); }

    static constexpr repr_tag_t decode_tag(data_word_t dw) {
        return static_cast<repr_tag_t>(dw & 0xFF);
    }

    // Encode and decode the inline tag and length into a single data word.
    static data_word_t encode_inline_length(uint8_t len) {
        return ((kMaxInlineCharCount - len) << 8u) | encode_tag(repr_tag_t::inlined);
    }

    static uint8_t decode_inline_length(data_word_t dw) {
        return kMaxInlineCharCount - static_cast<uint8_t>(dw >> 8);
    }

    repr_tag_t get_backing_type() const { return repr_.tag(); }

    // Set this equal to another string, perhaps copying it.
    void set_or_copy_from(const imstring &rhs);

    template <typename T, repr_tag_t Tag>
    struct pointer_base_repr_t {
        T *ptr;
        size_t len;

        // Padding sufficient to align our data word with inlined_repr_t.
        // Here we assume that wchar_t does not have stricter alignment than size_t or pointers,
        // which is reasonable.
        // Note this calculation is static_asserted in the .cpp file.
        static constexpr size_t kPaddingBytes =
            kMaxInlineCharCount * sizeof(wchar_t) - sizeof(T *) - sizeof(size_t);
        wchar_t padding[kPaddingBytes / sizeof(wchar_t)];

        data_word_t data;

        constexpr pointer_base_repr_t(T *ptr, size_t len)
            : ptr(ptr), len(len), padding{}, data(encode_tag(Tag)) {}
        static constexpr repr_tag_t tag() { return Tag; }
    };

    // A simple malloc'd array of chars, with a reference count.
    struct sharedarr_t {
        std::atomic<uint32_t> rc;
        wchar_t chars[1];

        // Allocate a sharedarr, with malloc.
        static sharedarr_t *create(const wchar_t *ptr, size_t len);

        // Increment our reference count.
        void increment_rc();

        // Decrement our reference count, and free self if it goes to 0.
        void decrement_rc();
    };

    struct inlined_repr_t {
        static constexpr repr_tag_t tag() { return repr_tag_t::inlined; }

        wchar_t storage[kMaxInlineCharCount];
        data_word_t data;

        // Note we zero out even unused inline chars, for sanity.
        inlined_repr_t(const wchar_t *ptr, size_t len)
            : storage{}, data(encode_inline_length(len)) {
            assert(len <= kMaxInlineCharCount && "length is too big");
            std::uninitialized_copy_n(ptr, len, storage);
        }
    };

    struct literal_repr_t : public pointer_base_repr_t<const wchar_t, repr_tag_t::literal> {
        constexpr literal_repr_t(const wchar_t *ptr, size_t len)
            : pointer_base_repr_t<const wchar_t, repr_tag_t::literal>(ptr, len) {}
    };

    struct unowned_repr_t : public pointer_base_repr_t<const wchar_t, repr_tag_t::unowned> {
        constexpr unowned_repr_t(const wchar_t *ptr, size_t len)
            : pointer_base_repr_t<const wchar_t, repr_tag_t::unowned>(ptr, len) {}
    };

    struct sharedarr_repr_t : public pointer_base_repr_t<sharedarr_t, repr_tag_t::sharedarr> {
        sharedarr_repr_t(const wchar_t *ptr, size_t len);
        ~sharedarr_repr_t();

        sharedarr_repr_t(const sharedarr_repr_t &);
        void operator=(const sharedarr_repr_t &) = delete;
    };

    // A pseudo-representation for accessing the shared data word.
    struct data_storage_t {
        wchar_t padding_[kMaxInlineCharCount];
        data_word_t data;
    };

    static constexpr literal_repr_t empty_literal() { return literal_repr_t{L"", 0}; }

    struct repr_t {
       public:
        // Construct as empty.
        constexpr repr_t() : literal_(empty_literal()) {}

        // Construct from our different representations.
        explicit constexpr repr_t(literal_repr_t v) : literal_(v) {}
        explicit repr_t(unowned_repr_t v) : unowned_(v) {}
        explicit repr_t(const inlined_repr_t &v) : inlined_(v) {}

        // \return our tag.
        repr_tag_t tag() const { return decode_tag(this->storage_.data); }

        // Helper to satisfy gcc.
        [[noreturn]] static void unreachable() {
            assert(0 && "Unrecahable");
            abort();
        }

        // Access our underlying pointer.
        const wchar_t *ptr() const {
            switch (tag()) {
                case repr_tag_t::inlined:
                    return inlined().storage;
                case repr_tag_t::literal:
                    return literal().ptr;
                case repr_tag_t::unowned:
                    return unowned().ptr;
                case repr_tag_t::sharedarr:
                    return sharedarr().ptr->chars;
            }
            unreachable();
            return 0;
        }

        // Access our underlying length.
        size_t length() const {
            switch (tag()) {
                case repr_tag_t::inlined:
                    return decode_inline_length(storage_.data);
                case repr_tag_t::literal:
                    return literal().len;
                case repr_tag_t::unowned:
                    return unowned().len;
                case repr_tag_t::sharedarr:
                    return sharedarr().len;
            }
            unreachable();
            return 0;
        }

        // Access our representations, asserting on the tag.
        literal_repr_t &literal() {
            check_tag(literal_);
            return literal_;
        }

        const literal_repr_t &literal() const {
            check_tag(literal_);
            return literal_;
        }

        unowned_repr_t &unowned() {
            check_tag(unowned_);
            return unowned_;
        }

        const unowned_repr_t &unowned() const {
            check_tag(unowned_);
            return unowned_;
        }

        inlined_repr_t &inlined() {
            check_tag(inlined_);
            return inlined_;
        }

        const inlined_repr_t &inlined() const {
            check_tag(inlined_);
            return inlined_;
        }

        sharedarr_repr_t &sharedarr() {
            check_tag(sharedarr_);
            return sharedarr_;
        }
        const sharedarr_repr_t &sharedarr() const {
            check_tag(sharedarr_);
            return sharedarr_;
        }

        // Set from our various representations.
        void set(const literal_repr_t &v) {
            destroy();
            new (&literal_) literal_repr_t(v);
        }

        void set(const unowned_repr_t &v) {
            destroy();
            new (&unowned_) unowned_repr_t(v);
        }

        void set(const inlined_repr_t &v) {
            destroy();
            new (&inlined_) inlined_repr_t(v);
        }

        void set(const sharedarr_repr_t &v) {
            destroy();
            new (&sharedarr_) sharedarr_repr_t(v);
        }

        // We have our own move and copy logic.
        repr_t(const repr_t &) = delete;
        repr_t &operator=(const repr_t &) = delete;
        repr_t(repr_t &&) = delete;
        repr_t &operator=(repr_t &&) = delete;
        ~repr_t();

       private:
        // Check that our tag agrees with the expected tag, which is given as T::tag().
        template <typename T>
        void check_tag(const T &) const {
            assert(this->tag() == T::tag() && "Wrong tag");
        }

        // Invoke the proper destructor of our field.
        // After invoking this, you must use placement new on a field to populate it.
        void destroy();

        union {
            literal_repr_t literal_;
            unowned_repr_t unowned_;
            inlined_repr_t inlined_;
            sharedarr_repr_t sharedarr_;
            data_storage_t storage_;
        };
    };
    repr_t repr_;

    // Helper constructors.
    enum class no_copy_t { nocopy };
    constexpr imstring(const wchar_t *str REF_BY_RET, size_t len, no_copy_t)
        : repr_(literal_repr_t{str, len}) {}

    // A place for static asserts to live. This does nothing.
    void static_asserts();

    friend imstring operator"" _im(const wchar_t *, size_t);
    friend void test_imstring();
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
