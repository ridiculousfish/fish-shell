// Various mostly unrelated utility functions related to parsing, loading and evaluating fish code.
#ifndef FISH_PARSE_UTIL_H
#define FISH_PARSE_UTIL_H

#include <stddef.h>

#include <vector>

#include "common.h"
#include "parse_tree.h"
#include "tokenizer.h"

namespace ast {
struct argument_t;
}

/// Handles slices: the square brackets in an expression like $foo[5..4]
/// \return the length of the slice starting at \p in, or 0 if there is no slice, or -1 on error.
/// This never accepts incomplete slices.
long parse_util_slice_length(const wchar_t *in);

/// Lightweight support for iterating over command substitutions.
/// This only finds top-level (not nested) cmdsubs.
struct cmdsubst_iterator_t {
    /// Construct with a nul-terminated string \p str and initial cursor location.
    cmdsubst_iterator_t(const wchar_t *str, size_t cursor, bool accept_incomplete)
        : base_(str), cursor_(cursor), accept_incomplete_(accept_incomplete) {
        assert(str && "Null string");
    }

    /// Construct with a nul-terminated string \p str, starting at 0.
    cmdsubst_iterator_t(const wchar_t *str, bool accept_incomplete)
        : cmdsubst_iterator_t(str, 0, accept_incomplete) {}

    /// Wrappers around wcstring. Note the caller must ensure the str stays alive.
    cmdsubst_iterator_t(const wcstring &str, bool accept_incomplete)
        : cmdsubst_iterator_t(str, 0, accept_incomplete) {}

    cmdsubst_iterator_t(const wcstring &str, size_t cursor, bool accept_incomplete)
        : cmdsubst_iterator_t(str.c_str(), cursor, accept_incomplete) {}

    /// Find the next cmdsub. This updates the below fields to reflect it.
    /// \return -1 on error, 0 if none, 1 on success.
    int next();

    /// offset of opening (.
    size_t paren_start{};

    // start of contents, extending to end.
    size_t contents_start{};

    // offset of closing ), or end of string if incomplete.
    size_t paren_end{};

    /// \return the size of the contents (the text of the cmdsub, not counting parens).
    size_t contents_size() const { return paren_end - contents_start; }

    /// \return the contents of the cmdsub by copying it into the provided storage.
    const wcstring &contents(wcstring *storage) const {
        storage->assign(base_, contents_start, contents_size());
        return *storage;
    }

    /// \return the contents of the most recently found cmdsub by allocating a new string.
    wcstring contents() const { return wcstring(base_, contents_start, contents_size()); }

    /// \return the cursor, where the next search for a cmdsub will start.
    size_t cursor() const { return cursor_; }

    /// Set the cursor to a new value. This controls where the next search for a cmdsub will start.
    void set_cursor(size_t cursor) { cursor_ = cursor; }

   private:
    // Initial string.
    const wchar_t *const base_;

    /// Location to begin the next search. Initially zero, later just-after closed paren.
    size_t cursor_{0};

    // Set if we should allow a missing closing paren.
    const bool accept_incomplete_;

    // Set when cursor exceeds the length of the string.
    bool finished_{false};
};

/// Find the beginning and end of the command substitution under the cursor. If no subshell is
/// found, the entire string is returned. If the current command substitution is not ended, i.e. the
/// closing parenthesis is missing, then the string from the beginning of the substitution to the
/// end of the string is returned.
///
/// \param buff the string to search for subshells
/// \param cursor_pos the position of the cursor
/// \param a the start of the searched string
/// \param b the end of the searched string
void parse_util_cmdsubst_extent(const wchar_t *buff, size_t cursor_pos, const wchar_t **a,
                                const wchar_t **b);

/// Find the beginning and end of the process definition under the cursor
///
/// \param buff the string to search for subshells
/// \param cursor_pos the position of the cursor
/// \param a the start of the process
/// \param b the end of the process
/// \param tokens the tokens in the process
void parse_util_process_extent(const wchar_t *buff, size_t cursor_pos, const wchar_t **a,
                               const wchar_t **b, std::vector<tok_t> *tokens);

/// Find the beginning and end of the job definition under the cursor
///
/// \param buff the string to search for subshells
/// \param cursor_pos the position of the cursor
/// \param a the start of the searched string
/// \param b the end of the searched string
void parse_util_job_extent(const wchar_t *buff, size_t cursor_pos, const wchar_t **a,
                           const wchar_t **b);

/// Find the beginning and end of the token under the cursor and the token before the current token.
/// Any combination of tok_begin, tok_end, prev_begin and prev_end may be null.
///
/// \param buff the string to search for subshells
/// \param cursor_pos the position of the cursor
/// \param tok_begin the start of the current token
/// \param tok_end the end of the current token
/// \param prev_begin the start o the token before the current token
/// \param prev_end the end of the token before the current token
void parse_util_token_extent(const wchar_t *buff, size_t cursor_pos, const wchar_t **tok_begin,
                             const wchar_t **tok_end, const wchar_t **prev_begin,
                             const wchar_t **prev_end);

/// Get the linenumber at the specified character offset.
int parse_util_lineno(const wchar_t *str, size_t offset);

/// Calculate the line number of the specified cursor position.
int parse_util_get_line_from_offset(const wcstring &str, size_t pos);

/// Get the offset of the first character on the specified line.
size_t parse_util_get_offset_from_line(const wcstring &str, int line);

/// Return the total offset of the buffer for the cursor position nearest to the specified poition.
size_t parse_util_get_offset(const wcstring &str, int line, long line_offset);

/// Return the given string, unescaping wildcard characters but not performing any other character
/// transformation.
wcstring parse_util_unescape_wildcards(const wcstring &str);

/// Checks if the specified string is a help option.
bool parse_util_argument_is_help(const wcstring &s);

/// Calculates information on the parameter at the specified index.
///
/// \param cmd The command to be analyzed
/// \param pos An index in the string which is inside the parameter
/// \return the type of quote used by the para
/// \param quote If not NULL, store the type of quote this parameter has, can be either ', " or \\0,
/// meaning the string is not quoted.
wchar_t parse_util_get_quote_type(const wcstring &cmd, const size_t pos);

/// Attempts to escape the string 'cmd' using the given quote type, as determined by the quote
/// character. The quote can be a single quote or double quote, or L'\0' to indicate no quoting (and
/// thus escaping should be with backslashes). Optionally do not escape tildes.
wcstring parse_util_escape_string_with_quote(const wcstring &cmd, wchar_t quote,
                                             bool no_tilde = false);

/// Given a string, parse it as fish code and then return the indents. The return value has the same
/// size as the string.
std::vector<int> parse_util_compute_indents(const wcstring &src);

/// Given a string, detect parse errors in it. If allow_incomplete is set, then if the string is
/// incomplete (e.g. an unclosed quote), an error is not returned and the PARSER_TEST_INCOMPLETE bit
/// is set in the return value. If allow_incomplete is not set, then incomplete strings result in an
/// error.
parser_test_error_bits_t parse_util_detect_errors(const wcstring &buff_src,
                                                  parse_error_list_t *out_errors = nullptr,
                                                  bool allow_incomplete = false);

/// Like parse_util_detect_errors but accepts an already-parsed ast.
/// The top of the ast is assumed to be a job list.
parser_test_error_bits_t parse_util_detect_errors(const ast::ast_t &ast, const wcstring &buff_src,
                                                  parse_error_list_t *out_errors);

/// Detect errors in the specified string when parsed as an argument list. Returns the text of an
/// error, or none if no error occurred.
maybe_t<wcstring> parse_util_detect_errors_in_argument_list(const wcstring &arg_list_src,
                                                            const wcstring &prefix = {});

/// Test if this argument contains any errors. Detected errors include syntax errors in command
/// substitutions, improperly escaped characters and improper use of the variable expansion
/// operator. This does NOT currently detect unterminated quotes.

parser_test_error_bits_t parse_util_detect_errors_in_argument(
    const ast::argument_t &arg, const wcstring &arg_src, parse_error_list_t *out_errors = nullptr);

/// Given a string containing a variable expansion error, append an appropriate error to the errors
/// list. The global_token_pos is the offset of the token in the larger source, and the dollar_pos
/// is the offset of the offending dollar sign within the token.
void parse_util_expand_variable_error(const wcstring &token, size_t global_token_pos,
                                      size_t dollar_pos, parse_error_list_t *out_errors);

#endif
