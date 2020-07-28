// Generic utilities library.
#include "config.h"  // IWYU pragma: keep

#include "util.h"

#include <errno.h>
#include <stddef.h>
#include <sys/time.h>
#include <wctype.h>

#include <atomic>
#include <cwchar>

#include "common.h"
#include "fallback.h"  // IWYU pragma: keep
#include "flog.h"
#include "wutil.h"     // IWYU pragma: keep

// Compare the strings to see if they begin with an integer that can be compared and return the
// result of that comparison.
static int wcsfilecmp_leading_digits(const wchar_t **a, const wchar_t **b) {
    const wchar_t *a_end, *b_end;

    long a_num = fish_wcstol(*a, &a_end, 10);
    if (errno > 0) return 0;  // invalid number -- fallback to simple string compare
    long b_num = fish_wcstol(*b, &b_end, 10);
    if (errno > 0) return 0;  // invalid number -- fallback to simple string compare

    if (a_num < b_num) return -1;
    if (a_num > b_num) return 1;
    *a = a_end;
    *b = b_end;
    return 0;
}

/// Compare two strings, representing file names, using "natural" ordering. This means that letter
/// case is ignored. It also means that integers in each string are compared based on the decimal
/// value rather than the string representation. It only handles base 10 integers and they can
/// appear anywhere in each string, including multiple integers. This means that a file name like
/// "0xAF0123" is treated as the literal "0xAF" followed by the integer 123.
///
/// The intent is to ensure that file names like "file23" and "file5" are sorted so that the latter
/// appears before the former.
///
/// This does not handle esoterica like Unicode combining characters. Nor does it use collating
/// sequences. Which means that an ASCII "A" will be less than an equivalent character with a higher
/// Unicode code point. In part because doing so is really hard without the help of something like
/// the ICU library. But also because file names might be in a different encoding than is used by
/// the current fish process which results in weird situations. This is basically a best effort
/// implementation that will do the right thing 99.99% of the time.
///
/// Returns: -1 if a < b, 0 if a == b, 1 if a > b.
int wcsfilecmp(const wchar_t *a, const wchar_t *b) {
    assert(a && b && "Null parameter");
    const wchar_t *orig_a = a;
    const wchar_t *orig_b = b;
    int retval = 0;  // assume the strings will be equal

    while (*a && *b) {
        if (iswdigit(*a) && iswdigit(*b)) {
            retval = wcsfilecmp_leading_digits(&a, &b);
            // If we know the strings aren't logically equal or we've reached the end of one or both
            // strings we can stop iterating over the chars in each string.
            if (retval || *a == 0 || *b == 0) break;
        }

        wint_t al = towupper(*a);
        wint_t bl = towupper(*b);
        // Sort dashes after Z - see #5634
        if (al == L'-') al = L'[';
        if (bl == L'-') bl = L'[';

        if (al < bl) {
            retval = -1;
            break;
        } else if (al > bl) {
            retval = 1;
            break;
        } else {
            a++;
            b++;
        }
    }

    if (retval != 0) return retval;  // we already know the strings aren't logically equal

    if (*a == 0) {
        if (*b == 0) {
            // The strings are logically equal. They may or may not be the same length depending on
            // whether numbers were present but that doesn't matter. Disambiguate strings that
            // differ by letter case or length. We don't bother optimizing the case where the file
            // names are literally identical because that won't occur given how this function is
            // used. And even if it were to occur (due to being reused in some other context) it
            // would be so rare that it isn't worth optimizing for.
            retval = std::wcscmp(orig_a, orig_b);
            return retval < 0 ? -1 : retval == 0 ? 0 : 1;
        }
        return -1;  // string a is a prefix of b and b is longer
    }

    assert(*b == 0);
    return 1;  // string b is a prefix of a and a is longer
}

/// wcsfilecmp, but frozen in time for glob usage.
int wcsfilecmp_glob(const wchar_t *a, const wchar_t *b) {
    assert(a && b && "Null parameter");
    const wchar_t *orig_a = a;
    const wchar_t *orig_b = b;
    int retval = 0;  // assume the strings will be equal

    while (*a && *b) {
        if (iswdigit(*a) && iswdigit(*b)) {
            retval = wcsfilecmp_leading_digits(&a, &b);
            // If we know the strings aren't logically equal or we've reached the end of one or both
            // strings we can stop iterating over the chars in each string.
            if (retval || *a == 0 || *b == 0) break;
        }

        wint_t al = towlower(*a);
        wint_t bl = towlower(*b);
        if (al < bl) {
            retval = -1;
            break;
        } else if (al > bl) {
            retval = 1;
            break;
        } else {
            a++;
            b++;
        }
    }

    if (retval != 0) return retval;  // we already know the strings aren't logically equal

    if (*a == 0) {
        if (*b == 0) {
            // The strings are logically equal. They may or may not be the same length depending on
            // whether numbers were present but that doesn't matter. Disambiguate strings that
            // differ by letter case or length. We don't bother optimizing the case where the file
            // names are literally identical because that won't occur given how this function is
            // used. And even if it were to occur (due to being reused in some other context) it
            // would be so rare that it isn't worth optimizing for.
            retval = wcscmp(orig_a, orig_b);
            return retval < 0 ? -1 : retval == 0 ? 0 : 1;
        }
        return -1;  // string a is a prefix of b and b is longer
    }

    assert(*b == 0);
    return 1;  // string b is a prefix of a and a is longer
}

/// Return microseconds since the epoch.
long long get_time() {
    struct timeval time_struct;
    gettimeofday(&time_struct, nullptr);
    return 1000000LL * time_struct.tv_sec + time_struct.tv_usec;
}

/// chdir_serializer_t is responsible for serializing calls to fchdir().
/// This is necessary because cwd must be correct during calls to fork() - there is no 'fork_at'.
struct chdir_serializer_t {
    struct data_t {
        /// The current working directory. This corresponds to the most recent *successful* call to
        /// fchdir().
        std::shared_ptr<const autoclose_fd_t> current{};

        /// Total number of locks on 'current'.
        /// The CWD is only permitted to change if lock_count is 0.
        uint32_t lock_count{0};

        /// A pair of counters for use in serializing threads.
        /// Each thread "takes a ticket" by postincrementing next_available, and only runs when it
        /// equals now_serving. The purpose of the tickets is to ensure the lock is fair: if two
        /// threads disagree on the CWD they should take turns. Note that the difference 'last_taken
        /// - now_serving' is the current number of waiters.
        uint64_t next_available{0};
        uint64_t now_serving{0};
    };

    /// Data protected by the lock.
    owning_lock<data_t> data_{};

    /// A condition variable for waiting for the cwd to be released.
    /// The associated mutex is the one protecting 'data'.
    std::condition_variable condition_{};

    /// Set the cwd to a given value, waiting until it's our turn to do so.
    /// \return 0 on success, an errno value if fchdir() fails.
    int lock_cwd(const std::shared_ptr<const autoclose_fd_t> &dir_fd, fchdir_lock_t *out_lock);

    /// Mark that a user of the CWD is finished.
    void release_cwd_lock();

    /// Advance the now_serving ticket, if there are no locks on it.
    void try_advance_ticket(acquired_lock<data_t> &data);

    /// The shared chdir serializer.
    static chdir_serializer_t *const shared;
};

void chdir_serializer_t::try_advance_ticket(acquired_lock<data_t> &data) {
    assert(data->now_serving <= data->next_available && "tickets should be monotone increasing");
    // Only need to post if someone is waiting.
    if (data->lock_count == 0 && data->now_serving < data->next_available) {
        data->now_serving += 1;
        // TODO: faster to post without holding the mutex.
        condition_.notify_all();
    }
}

int chdir_serializer_t::lock_cwd(const std::shared_ptr<const autoclose_fd_t> &dir_fd,
                                 fchdir_lock_t *out_lock) {
    auto data = data_.acquire();

    // Very common fast path: if nobody is waiting and current cwd already agrees, we can simply
    // return 0 (perhaps bumping the lock count).
    // This way multiple users can share the lock if they agree on the cwd.
    if (data->current == dir_fd && data->now_serving == data->next_available) {
        if (out_lock) {
            assert(!out_lock->locked && "Should not already be locked");
            out_lock->locked = true;
            data->lock_count += 1;
        }
        return 0;
    }

    // Take a ticket and wait until it's our turn.
    assert(data->now_serving <= data->next_available && "tickets should be monotone increasing");
    const uint64_t ticket = data->next_available++;
    while (data->now_serving != ticket) {
        condition_.wait(data.get_lock());
    }

    // It's our turn. Invoke fchdir() if we are not already in the right directory.
    // We may want to change the lock count, it has to be zero!
    assert(data->lock_count == 0 && "Should be no locks");
    int err = 0;
    if (data->current != dir_fd) {
        // Loop on EINTR.
        do {
            int res = fchdir(dir_fd->fd());
            err = res ? errno : 0;
        } while (err == EINTR);

        // Save the directory if fchdir succeeded.
        if (!err) data->current = dir_fd;
    }

    // Bump the lock count if there was no error and if requested.
    if (err == 0 && out_lock != nullptr) {
        assert(!out_lock->locked && "Should not already be locked");
        out_lock->locked = true;
        data->lock_count += 1;
    }
    try_advance_ticket(data);
    if (err) {
        errno = err;
        return -1;
    }
    return 0;
}

void chdir_serializer_t::release_cwd_lock() {
    auto data = data_.acquire();
    assert(data->lock_count > 0 && "Lock count should be > 0");
    data->lock_count -= 1;
    try_advance_ticket(data);
}

/// Leaked to avoid shutdown dtor registration.
chdir_serializer_t *const chdir_serializer_t::shared = new chdir_serializer_t();

fchdir_lock_t::~fchdir_lock_t() {
    if (locked) chdir_serializer_t::shared->release_cwd_lock();
}

int locking_fchdir(const std::shared_ptr<const autoclose_fd_t> &dir_fd, fchdir_lock_t *out_lock) {
    return chdir_serializer_t::shared->lock_cwd(dir_fd, out_lock);
}
