#ifndef FISH_IO_H
#define FISH_IO_H

#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>

#include <vector>
// Note that we have to include something to get any _LIBCPP_VERSION defined so we can detect libc++
// So it's key that vector go above. If we didn't need vector for other reasons, we might include
// ciso646, which does nothing
#if defined(_LIBCPP_VERSION) || __cplusplus > 199711L
// C++11 or libc++ (which is a C++11-only library, but the memory header works OK in C++03)
#include <memory>
using std::shared_ptr;
#else
// C++03 or libstdc++
#include <tr1/memory>
using std::tr1::shared_ptr;
#endif

#include "common.h"
#include "env.h"

enum class separation_type_t { inferred, explicitly };

/// Support for explicitly separated output.
template <typename StringType>
class separated_buffer_t {
    struct element_t {
        StringType contents;
        separation_type_t separation;

        element_t(StringType contents, separation_type_t sep)
            : contents(std::move(contents)), separation(sep) {}

        bool is_explicitly_separated() const { return separation == separation_type_t::explicitly; }
    };

    /// Limit on how much data we'll buffer. Zero means no limit.
    size_t buffer_limit_;

    /// Current size of all contents.
    size_t contents_size_{0};

    /// List of buffer elements.
    std::vector<element_t> elements_;

    /// True if we're discarding input because our buffer_limit has been exceeded.
    bool discard = false;

    bool try_add_size(size_t delta) {
        if (discard) return false;
        contents_size_ += delta;
        if (contents_size_ < delta) {
            // Overflow!
            set_discard();
            return false;
        }
        if (buffer_limit_ > 0 && contents_size_ > buffer_limit_) {
            set_discard();
            return false;
        }
        return true;
    }

    /// Disable copying.
    separated_buffer_t(const separated_buffer_t &) = delete;
    void operator=(const separated_buffer_t &) = delete;

public:
    size_t size() const { return contents_size_; }

    separated_buffer_t(size_t limit) : buffer_limit_(limit) {}

    bool discarded() const { return discard; }

    void set_discard() {
        elements_.clear();
        contents_size_ = 0;
        discard = true;
    }

    size_t limit() const { return buffer_limit_; }

    StringType serialized_with_newlines() const {
        StringType result;
        result.reserve(size());
        for (const auto &elem : elements_) {
            result.append(elem.contents);
            if (elem.is_explicitly_separated()) {
                result.push_back('\n');
            }
        }
        return result;
    }

    const std::vector<element_t> &elements() const { return elements_; }

    template <typename Iterator>
    void append(Iterator begin, Iterator end, separation_type_t sep = separation_type_t::inferred) {
        if (!try_add_size(std::distance(begin, end))) return;
        // Try merging with the last element.
        if (sep == separation_type_t::inferred && !elements_.empty() && !elements_.back().is_explicitly_separated()) {
            elements_.back().contents.append(begin, end);
        } else {
            elements_.emplace_back(StringType(begin, end), sep);
        }
    }

    void append(const StringType &c, separation_type_t sep = separation_type_t::inferred) { append(c.begin(), c.end(), sep); }

    void append_explicitly_separated_output(std::vector<StringType> strs) {
        for (const auto &s : strs) {
            if (!try_add_size(s.size())) return;
        }
        for (auto &s : strs) {
            elements_.emplace_back(std::move(s), separation_type_t::explicitly);
        }
    }

    // Given that this is a narrow stream, convert a wide stream \p rhs to narrow and then append
    // it.
    template <typename RHSStringType>
    void append_wide_buffer(const separated_buffer_t<RHSStringType> &rhs) {
        for (const auto &rhs_elem : rhs.elements()) {
            append(wcs2string(rhs_elem.contents), rhs_elem.separation);
        }
    }
};

/// Describes what type of IO operation an io_data_t represents.
enum io_mode_t { IO_FILE, IO_PIPE, IO_FD, IO_BUFFER, IO_CLOSE };

/// Represents an FD redirection.
class io_data_t {
   private:
    // No assignment or copying allowed.
    io_data_t(const io_data_t &rhs);
    void operator=(const io_data_t &rhs);

   protected:
    io_data_t(io_mode_t m, int f) : io_mode(m), fd(f) {}

   public:
    /// Type of redirect.
    const io_mode_t io_mode;
    /// FD to redirect.
    const int fd;

    virtual void print() const = 0;
    virtual ~io_data_t() = 0;
};

class io_close_t : public io_data_t {
   public:
    explicit io_close_t(int f) : io_data_t(IO_CLOSE, f) {}

    void print() const override;
};

class io_fd_t : public io_data_t {
   public:
    /// fd to redirect specified fd to. For example, in 2>&1, old_fd is 1, and io_data_t::fd is 2.
    const int old_fd;

    /// Whether this redirection was supplied by a script. For example, 'cmd <&3' would have
    /// user_supplied set to true. But a redirection that comes about through transmogrification
    /// would not.
    const bool user_supplied;

    void print() const override;

    io_fd_t(int f, int old, bool us) : io_data_t(IO_FD, f), old_fd(old), user_supplied(us) {}
};

class io_file_t : public io_data_t {
   public:
    /// Filename, malloc'd. This needs to be used after fork, so don't use wcstring here.
    const char *const filename_cstr;
    /// file creation flags to send to open.
    const int flags;

    void print() const override;

    io_file_t(int f, const wcstring &fname, int fl = 0)
        : io_data_t(IO_FILE, f), filename_cstr(wcs2str(fname)), flags(fl) {}

    ~io_file_t() override { free((void *)filename_cstr); }
};

class io_pipe_t : public io_data_t {
   protected:
    io_pipe_t(io_mode_t m, int f, bool i) : io_data_t(m, f), is_input(i) {
        pipe_fd[0] = pipe_fd[1] = -1;
    }

   public:
    int pipe_fd[2];
    const bool is_input;

    void print() const override;

    io_pipe_t(int f, bool i) : io_data_t(IO_PIPE, f), is_input(i) { pipe_fd[0] = pipe_fd[1] = -1; }
};

class io_chain_t;
class output_stream_t;
class io_buffer_t : public io_pipe_t {
   private:
    separated_buffer_t<std::string> buffer_;

    explicit io_buffer_t(int f, size_t limit)
        : io_pipe_t(IO_BUFFER, f, false /* not input */),
          buffer_(limit) {}

   public:
    void print() const override;

    ~io_buffer_t() override;

    /// Function to append to the buffer.
    void append(const char *ptr, size_t count) { buffer_.append(ptr, ptr + count); }

    /// Function to get the contents of the buffer. TODO: less copying.
    std::string serialized_with_newlines() const {
        return buffer_.serialized_with_newlines();
    }

    /// Function that returns true if we discarded the input because there was too much data.
    bool output_discarded() { return buffer_.discarded(); }

    /// Function to explicitly put the object in discard mode. Meant to be used when moving
    /// the results from an output_stream_t to an io_buffer_t.
    void set_discard() {
        buffer_.set_discard();
    }

    /// This is used to transfer the buffer limit for this object to a output_stream_t object.
    size_t get_buffer_limit() const { return buffer_.limit(); }

    /// return the size of the out buffer.
    size_t buffer_size() const { return buffer_.size(); }

    /// Ensures that the pipes do not conflict with any fd redirections in the chain.
    bool avoid_conflicts_with_io_chain(const io_chain_t &ios);

    /// Close output pipe, and read from input pipe until eof.
    void read();

    /// Appends data from a given output_stream_t.
    /// Marks the receiver as discarded if the stream was discarded.
    void append_from_stream(const output_stream_t &stream);

    /// Create a IO_BUFFER type io redirection, complete with a pipe and a vector<char> for output.
    /// The default file descriptor used is STDOUT_FILENO for buffering.
    ///
    /// \param fd the fd that will be mapped in the child process, typically STDOUT_FILENO
    /// \param conflicts A set of IO redirections. The function ensures that any pipe it makes does
    /// not conflict with an fd redirection in this list.
    static shared_ptr<io_buffer_t> create(int fd, const io_chain_t &conflicts,
                                          size_t buffer_limit = 0);
};

class io_chain_t : public std::vector<shared_ptr<io_data_t> > {
   public:
    io_chain_t();
    explicit io_chain_t(const shared_ptr<io_data_t> &);

    void remove(const shared_ptr<const io_data_t> &element);
    void push_back(const shared_ptr<io_data_t> &element);
    void push_front(const shared_ptr<io_data_t> &element);
    void append(const io_chain_t &chain);

    shared_ptr<const io_data_t> get_io_for_fd(int fd) const;
    shared_ptr<io_data_t> get_io_for_fd(int fd);
};

/// Return the last io redirection in the chain for the specified file descriptor.
shared_ptr<const io_data_t> io_chain_get(const io_chain_t &src, int fd);
shared_ptr<io_data_t> io_chain_get(io_chain_t &src, int fd);

/// Given a pair of fds, if an fd is used by the given io chain, duplicate that fd repeatedly until
/// we find one that does not conflict, or we run out of fds. Returns the new fds by reference,
/// closing the old ones. If we get an error, returns false (in which case both fds are closed and
/// set to -1).
bool pipe_avoid_conflicts_with_io_chain(int fds[2], const io_chain_t &ios);

/// Class representing the output that a builtin can generate.
class output_stream_t {
   private:
    friend io_buffer_t;

    /// Storage for our data.
    separated_buffer_t<wcstring> buffer_;

    // No copying.
    output_stream_t(const output_stream_t &s) = delete;
    void operator=(const output_stream_t &s) = delete;

   public:
    output_stream_t(size_t buffer_limit) : buffer_(buffer_limit) {}

    void append(const wcstring &s) {
        buffer_.append(s);
    }

    void append(const wchar_t *s) { append(s, wcslen(s)); }

    void append(wchar_t s) { append(&s, 1); }

    void append(const wchar_t *s, size_t amt) { buffer_.append(s, s + amt); }

    void push_back(wchar_t c) { append(c); }

    void append_format(const wchar_t *format, ...) {
        va_list va;
        va_start(va, format);
        append_formatv(format, va);
        va_end(va);
    }

    void append_formatv(const wchar_t *format, va_list va) {
        buffer_.append(vformat_string(format, va));
    }

    wcstring buffer() const { return buffer_.serialized_with_newlines(); }

    /// Black magic. Adds explicitly separated output. This is used by `string split0`.
    void append_explicitly_separated_output(wcstring_list_t output) {
        buffer_.append_explicitly_separated_output(std::move(output));
    }

    /// Function that returns true if we discarded the input because there was too much data.
    bool output_discarded() const { return buffer_.discarded(); }

    bool empty() const { return buffer_.size() == 0; }
};

struct io_streams_t {
    output_stream_t out;
    output_stream_t err;

    // fd representing stdin. This is not closed by the destructor.
    int stdin_fd;

    // Whether stdin is "directly redirected," meaning it is the recipient of a pipe (foo | cmd) or
    // direct redirection (cmd < foo.txt). An "indirect redirection" would be e.g. begin ; cmd ; end
    // < foo.txt
    bool stdin_is_directly_redirected;

    // Indicates whether stdout and stderr are redirected (e.g. to a file or piped).
    bool out_is_redirected;
    bool err_is_redirected;

    // Actual IO redirections. This is only used by the source builtin. Unowned.
    const io_chain_t *io_chain;

    io_streams_t(size_t read_limit)
        : out(read_limit),
          err(read_limit),
          stdin_fd(-1),
          stdin_is_directly_redirected(false),
          out_is_redirected(false),
          err_is_redirected(false),
          io_chain(NULL) {}
};

#if 0
// Print debug information about the specified IO redirection chain to stderr.
void io_print(const io_chain_t &chain);
#endif

#endif
