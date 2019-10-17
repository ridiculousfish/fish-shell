// Generic utilities library.
#include "config.h"  // IWYU pragma: keep

#include "util.h"

#include <errno.h>
#include <stddef.h>
#include <sys/time.h>
#include <wctype.h>

#include <cwchar>

#include "common.h"
#include "fallback.h"  // IWYU pragma: keep
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

/// Return microseconds since the epoch.
long long get_time() {
    struct timeval time_struct;
    gettimeofday(&time_struct, 0);
    return 1000000ll * time_struct.tv_sec + time_struct.tv_usec;
}

int locking_fchdir(const std::shared_ptr<const autoclose_fd_t> &dir_fd,
                   std::unique_lock<std::mutex> *out_lock) {
    static std::mutex s_lock;

    // Take the lock, perhaps giving it to the caller.
    // TODO: it would be nice to make this a counting lock that can be held by multiple callers so
    // long as they all agree on the cwd. In practice we won't have different threads with different
    // wds often.
    std::unique_lock<std::mutex> locker(s_lock);
    if (out_lock) {
        *out_lock = std::move(locker);
    }

    // Change directories if needed.
    // Note s_current_cwd is protected by the lock.
    static std::shared_ptr<const autoclose_fd_t> s_current_cwd;
    if (s_current_cwd == dir_fd) {
        // The cwd has not changed.
        return 0;
    }

    int ret = fchdir(dir_fd->fd());
    if (ret == 0) {
        s_current_cwd = dir_fd;
    }
    return ret;
}
