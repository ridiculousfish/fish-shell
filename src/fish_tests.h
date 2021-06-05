#include <cwchar>

// The odd formulation of these macros is to avoid "multiple unary operator" warnings from oclint
// were we to use the more natural "if (!(e)) err(..." form. We have to do this because the rules
// for the C preprocessor make it practically impossible to embed a comment in the body of a macro.
#define do_test(e)                                             \
    do {                                                       \
        if (e) {                                               \
            ;                                                  \
        } else {                                               \
            err(L"Test failed on line %lu: %s", __LINE__, #e); \
        }                                                      \
    } while (0)

#define do_test_from(e, from)                                                   \
    do {                                                                        \
        if (e) {                                                                \
            ;                                                                   \
        } else {                                                                \
            err(L"Test failed on line %lu (from %lu): %s", __LINE__, from, #e); \
        }                                                                       \
    } while (0)

#define do_test1(e, msg)                                           \
    do {                                                           \
        if (e) {                                                   \
            ;                                                      \
        } else {                                                   \
            err(L"Test failed on line %lu: %ls", __LINE__, (msg)); \
        }                                                          \
    } while (0)

#define system_assert(command)                                     \
    if (system(command)) {                                         \
        err(L"Non-zero result on line %d: %s", __LINE__, command); \
    }

/// Report an error.
void err(const wchar_t *fmt, ...);

/// Print formatted output.
void say(const wchar_t *fmt, ...);

// Indicate if we should test the given function. Either we test everything (all arguments) or we
// run only tests that have a prefix in s_arguments.
// If \p default_on is set, then allow no args to run this test by default.
bool should_test_function(const char *func_name, bool default_on = true);
