#ifndef FISH_ERRORS_H
#define FISH_ERRORS_H

#include "config.h"

#include <errno.h>

#include "maybe.h"

void wperror_code(const wchar_t *, int);

/// A type which represents an error or a value.
/// Errors are taken from errno.h.
template <typename T>
class result_t {
   public:
    /// \return true if this result errored.
    bool errored() const { return error_ != 0; }

    /// \return true if this result did not error.
    bool is_ok() const { return !errored(); }

    /// \return the error code, or 0 if none.
    int code() const { return error_; }

    /// If this produced an error, then do the equivalent of wperror().
    /// Otherwise do nothing.
    void check_print(const wchar_t *syscall = L"") {
        if (errored()) wperror_code(syscall, code());
    }

    /// \return the error code as a maybe.
    /// This enables a nice idiom:
    ///   if (auto err = func().as_err())
    maybe_t<int> as_err() {
        if (errored()) return code();
        return none();
    }

    /// \return the value, assuming this is not an error.
    const T &value() const {
        assert(!errored() && "result is an error");
        return *value_;
    }

    /// \return the value, assuming this is not an error.
    T &value() {
        assert(!errored() && "result is an error");
        return *value_;
    }

    /// Acquire the value, transferring ownership to the caller.
    T acquire() {
        assert(!errored() && "result is an error");
        return value_.acquire();
    }

    /// Dereference support.
    const T *operator->() const { return &value(); }
    T *operator->() { return &value(); }
    const T &operator*() const { return value(); }
    T &operator*() { return value(); }

    /// Construct from a value.
    /* implicit */ result_t(T &&v) : value_(std::move(v)) {}
    /* implicit */ result_t(const T &v) : value_(v) {}

    /// Construct from an error code value.
    static result_t from_code(int err) {
        assert(err != 0 && "0 is not a valid error");
        result_t res{};
        res.error_ = err;
        return res;
    }

    /// Tricky: construct from a result_t<void>.
    /// This allows the following idiom: return error_t::from_errno().
    /* implicit */ inline result_t(result_t<void> v);

    /// Construct from errno.
    static result_t from_errno() { return from_code(errno); }

   private:
    /// Private default constructor which has neither an error nor a value.
    /// Prefer to construct by passing in a value instead.
    result_t() = default;

    /// The value which was returned.
    maybe_t<T> value_;

    /// If nonzero, the errno value.
    int error_{0};

} __warn_unused_type;

/// Void specialization. There is no value to store here.
template <>
class result_t<void> {
   public:
    /// \return true if this result errored.
    bool errored() const { return error_ != 0; }

    /// \return true if this result did not error.
    bool is_ok() const { return !errored(); }

    /// \return the error code, or 0 if none.
    int code() const { return error_; }

    /// \return the error code as a maybe.
    /// This enables a nice idiom:
    ///   if (auto err = func().as_err())
    maybe_t<int> as_err() {
        if (errored()) return code();
        return none();
    }

    /// If this produced an error, then do the equivalent of wperror().
    /// Otherwise do nothing.
    void check_print(const wchar_t *syscall = L"") {
        if (errored()) wperror_code(syscall, code());
    }

    /// Default construction is no error.
    result_t() = default;

    /// Construct an "OK" value.
    static result_t ok() { return result_t{}; }

    /// Construct from an error value.
    static result_t from_code(int err) {
        assert(err != 0 && "0 is not a valid error");
        result_t res{};
        res.error_ = err;
        return res;
    }

    /// Construct from errno.
    static result_t from_errno() { return from_code(errno); }

   private:
    /// If nonzero, the errno value.
    int error_{0};

} __warn_unused_type;

/// Convenience type for a function which just returns an error.
using error_t = result_t<void>;

template <typename T>
inline result_t<T>::result_t(error_t v) : error_(v.code()) {}

#endif
