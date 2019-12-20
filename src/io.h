#ifndef FISH_IO_H
#define FISH_IO_H

#include <pthread.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>

#include <atomic>
#include <future>
#include <memory>
#include <mutex>
#include <vector>

#include "common.h"
#include "env.h"
#include "global_safety.h"
#include "maybe.h"
#include "redirection.h"

using std::shared_ptr;

/// A simple set of FDs.
struct fd_set_t {
    std::vector<bool> fds;

    void add(int fd) {
        assert(fd >= 0 && "Invalid fd");
        if ((size_t)fd >= fds.size()) {
            fds.resize(fd + 1);
        }
        fds[fd] = true;
    }

    bool contains(int fd) const {
        assert(fd >= 0 && "Invalid fd");
        return (size_t)fd < fds.size() && fds[fd];
    }
};

/// separated_buffer_t is composed of a sequence of elements, some of which may be explicitly
/// separated (e.g. through string spit0) and some of which the separation is inferred. This enum
/// tracks the type.
enum class separation_type_t {
    /// This element's separation should be inferred, e.g. through IFS.
    inferred,
    /// This element was explicitly separated and should not be separated further.
    explicitly
};

/// A separated_buffer_t contains a list of elements, some of which may be separated explicitly and
/// others which must be separated further by the user (e.g. via IFS).
template <typename StringType>
class separated_buffer_t {
   public:
    struct element_t {
        StringType contents;
        separation_type_t separation;

        element_t(StringType contents, separation_type_t sep)
            : contents(std::move(contents)), separation(sep) {}

        bool is_explicitly_separated() const { return separation == separation_type_t::explicitly; }
    };

   private:
    /// Limit on how much data we'll buffer. Zero means no limit.
    size_t buffer_limit_;

    /// Current size of all contents.
    size_t contents_size_{0};

    /// List of buffer elements.
    std::vector<element_t> elements_;

    /// True if we're discarding input because our buffer_limit has been exceeded.
    bool discard = false;

    /// Mark that we are about to add the given size \p delta to the buffer. \return true if we
    /// succeed, false if we exceed buffer_limit.
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

    /// separated_buffer_t may not be copied.
    separated_buffer_t(const separated_buffer_t &) = delete;
    void operator=(const separated_buffer_t &) = delete;

   public:
    /// Construct a separated_buffer_t with the given buffer limit \p limit, or 0 for no limit.
    separated_buffer_t(size_t limit) : buffer_limit_(limit) {}

    /// \return the buffer limit size, or 0 for no limit.
    size_t limit() const { return buffer_limit_; }

    /// \return the contents size.
    size_t size() const { return contents_size_; }

    /// \return whether the output has been discarded.
    bool discarded() const { return discard; }

    /// Mark the contents as discarded.
    void set_discard() {
        elements_.clear();
        contents_size_ = 0;
        discard = true;
    }

    void reset_discard() { discard = false; }

    /// Serialize the contents to a single string, where explicitly separated elements have a
    /// newline appended.
    StringType newline_serialized() const {
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

    /// \return the list of elements.
    const std::vector<element_t> &elements() const { return elements_; }

    /// Append an element with range [begin, end) and the given separation type \p sep.
    template <typename Iterator>
    void append(Iterator begin, Iterator end, separation_type_t sep = separation_type_t::inferred) {
        if (!try_add_size(std::distance(begin, end))) return;
        // Try merging with the last element.
        if (sep == separation_type_t::inferred && !elements_.empty() &&
            !elements_.back().is_explicitly_separated()) {
            elements_.back().contents.append(begin, end);
        } else {
            elements_.emplace_back(StringType(begin, end), sep);
        }
    }

    /// Append a string \p str with the given separation type \p sep.
    void append(const StringType &str, separation_type_t sep = separation_type_t::inferred) {
        append(str.begin(), str.end(), sep);
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
enum class io_mode_t {
    /// A redirection to a file, like '> /tmp/file.txt'
    file,

    /// A pipe redirection. Note these come in pairs.
    pipe,

    /// An fd redirection like '1>&2'.
    fd,

    /// A close redirection like '1>&-'.
    close,

    /// A special "bufferfill" redirection. This is a write end of a pipe such that, when written
    /// to, it fills an io_buffer_t.
    bufferfill
};

class io_buffer_t;
struct io_data_t;

using io_data_ref_t = std::shared_ptr<const io_data_t>;

/// io_data_t represents a redirection or pipe.
struct io_data_t final {
    /// The type of redirection.
    const io_mode_t io_mode;

    /// Which fd is being redirected.
    /// For example in a | b, fd would be 1 (STDOUT_FILENO).
    const int fd;

    /// /// The fd which gets dup2'd to 'fd', or -1 if this is a 'close' mode.
    const int old_fd;

    /// Create a close redirection, for example 1>&-
    static io_data_ref_t make_close(int fd);

    /// Create an fd redirection. For example 1>&2 would pass 1, 2.
    static io_data_ref_t make_fd(int fd, int old);

    /// Create a redirection to an opened file or a pipe, which must not be invalid.
    /// The result takes ownership of the file.
    static io_data_ref_t make_file(int fd, autoclose_fd_t file);

    /// Make a pipe. This is the same as make_file except it's clear it's for a pipe.
    /// The result takes ownership of the file.
    static io_data_ref_t make_pipe(int fd, autoclose_fd_t pipe);

    /// Create a bufferfill which, when written to, fills the buffer with its contents.
    /// \conflicts is used to ensure that none of the pipes we create overlap with a pipe that the
    /// user has requested. Bufferfills always target STDOUT_FILENO. \returns nullptr on failure,
    /// e.g. too many open fds.
    /// Note bufferfills alone are "mutable" so this does not return a const pointer.
    static std::shared_ptr<io_data_t> make_bufferfill(const fd_set_t &conflicts,
                                                      size_t buffer_limit = 0);

    /// Finish a bufferfill. Reset the receiver (possibly closing the write end of the pipe) and
    /// complete the fillthread. \return the filled buffer.
    static std::shared_ptr<io_buffer_t> finish_bufferfill(std::shared_ptr<io_data_t> &&filler);

    /// Return the buffer for a bufferfill.
    const std::shared_ptr<io_buffer_t> &buffer() const;

    /// No assignment or copying allowed.
    io_data_t(const io_data_t &rhs) = delete;
    void operator=(const io_data_t &rhs) = delete;

    /// Exposed only for make_shared; do not use directly.
    io_data_t(io_mode_t m, int fd, int old_fd, autoclose_fd_t old_fd_owner = autoclose_fd_t{},
              std::shared_ptr<io_buffer_t> buffer = {});

    ~io_data_t();

   private:
    friend io_chain_t;

    // If we own old_fd, then we ensure it gets closed here.
    const autoclose_fd_t old_fd_owner_{};

    /// If we are filling a buffer, that buffer.
    const std::shared_ptr<io_buffer_t> buffer_;

    void print() const;
};

class output_stream_t;

/// An io_buffer_t is a buffer which can populate itself by reading from an fd.
/// It is not an io_data_t.
class io_buffer_t {
   private:
    friend io_data_t;

    /// Buffer storing what we have read.
    separated_buffer_t<std::string> buffer_;

    /// Atomic flag indicating our fillthread should shut down.
    relaxed_atomic_bool_t shutdown_fillthread_{false};

    /// The future allowing synchronization with the background fillthread, if the fillthread is
    /// running. The fillthread fulfills the corresponding promise when it exits.
    std::future<void> fillthread_waiter_{};

    /// Lock for appending.
    std::mutex append_lock_{};

    /// Called in the background thread to run it.
    void run_background_fillthread(autoclose_fd_t readfd);

    /// Begin the background fillthread operation, reading from the given fd.
    void begin_background_fillthread(autoclose_fd_t readfd);

    /// End the background fillthread operation.
    void complete_background_fillthread();

    /// Helper to return whether the fillthread is running.
    bool fillthread_running() const { return fillthread_waiter_.valid(); }

   public:
    explicit io_buffer_t(size_t limit) : buffer_(limit) {
        // Explicitly reset the discard flag because we share this buffer.
        buffer_.reset_discard();
    }

    ~io_buffer_t();

    /// Access the underlying buffer.
    /// This requires that the background fillthread be none.
    const separated_buffer_t<std::string> &buffer() const {
        assert(!fillthread_running() && "Cannot access buffer during background fill");
        return buffer_;
    }

    /// Function to append to the buffer.
    void append(const char *ptr, size_t count) {
        scoped_lock locker(append_lock_);
        buffer_.append(ptr, ptr + count);
    }

    /// Appends data from a given output_stream_t.
    /// Marks the receiver as discarded if the stream was discarded.
    void append_from_stream(const output_stream_t &stream);
};

using io_data_ref_t = std::shared_ptr<const io_data_t>;

class io_chain_t : public std::vector<io_data_ref_t> {
   public:
    using std::vector<io_data_ref_t>::vector;
    // user-declared ctor to allow const init. Do not default this, it will break the build.
    io_chain_t() {}

    void remove(const io_data_ref_t &element);
    void push_back(io_data_ref_t element);
    void append(const io_chain_t &chain);

    /// \return the last io redirection in the chain for the specified file descriptor, or nullptr
    /// if none.
    io_data_ref_t io_for_fd(int fd) const;

    /// Attempt to resolve a list of redirection specs to IOs, appending to 'this'.
    /// \return true on success, false on error, in which case an error will have been printed.
    bool append_from_specs(const redirection_spec_list_t &specs, const wcstring &pwd);

    /// Output debugging information to stderr.
    void print() const;

    /// \return the set of redirected FDs.
    fd_set_t fd_set() const;
};

/// Helper type returned from making autoclose pipes.
struct autoclose_pipes_t {
    /// Read end of the pipe.
    autoclose_fd_t read;

    /// Write end of the pipe.
    autoclose_fd_t write;

    autoclose_pipes_t() = default;
    autoclose_pipes_t(autoclose_fd_t r, autoclose_fd_t w)
        : read(std::move(r)), write(std::move(w)) {}
};
/// Call pipe(), populating autoclose fds, avoiding conflicts.
/// The pipes are marked CLO_EXEC.
/// \return pipes on success, none() on error.
maybe_t<autoclose_pipes_t> make_autoclose_pipes(const fd_set_t &fdset);

/// If the given fd is present in \p fdset, duplicates it repeatedly until an fd not used in the set
/// is found or we run out. If we return a new fd or an error, closes the old one. If \p cloexec is
/// set, any fd created is marked close-on-exec. \returns -1 on failure (in which case the given fd
/// is still closed).
autoclose_fd_t move_fd_to_unused(autoclose_fd_t fd, const fd_set_t &fdset, bool cloexec = true);

/// Class representing the output that a builtin can generate.
class output_stream_t {
   private:
    /// Storage for our data.
    separated_buffer_t<wcstring> buffer_;

    // No copying.
    output_stream_t(const output_stream_t &s) = delete;
    void operator=(const output_stream_t &s) = delete;

   public:
    output_stream_t(size_t buffer_limit) : buffer_(buffer_limit) {}

    void append(const wcstring &s) { buffer_.append(s.begin(), s.end()); }

    separated_buffer_t<wcstring> &buffer() { return buffer_; }

    const separated_buffer_t<wcstring> &buffer() const { return buffer_; }

    void append(const wchar_t *s) { append(s, std::wcslen(s)); }

    void append(wchar_t s) { append(&s, 1); }

    void append(const wchar_t *s, size_t amt) { buffer_.append(s, s + amt); }

    void push_back(wchar_t c) { append(c); }

    void append_format(const wchar_t *format, ...) {
        va_list va;
        va_start(va, format);
        append_formatv(format, va);
        va_end(va);
    }

    void append_formatv(const wchar_t *format, va_list va) { append(vformat_string(format, va)); }

    wcstring contents() const { return buffer_.newline_serialized(); }
};

struct io_streams_t {
    output_stream_t out;
    output_stream_t err;

    // fd representing stdin. This is not closed by the destructor.
    int stdin_fd{-1};

    // Whether stdin is "directly redirected," meaning it is the recipient of a pipe (foo | cmd) or
    // direct redirection (cmd < foo.txt). An "indirect redirection" would be e.g. begin ; cmd ; end
    // < foo.txt
    bool stdin_is_directly_redirected{false};

    // Indicates whether stdout and stderr are redirected (e.g. to a file or piped).
    bool out_is_redirected{false};
    bool err_is_redirected{false};

    // Actual IO redirections. This is only used by the source builtin. Unowned.
    const io_chain_t *io_chain{nullptr};

    // io_streams_t cannot be copied.
    io_streams_t(const io_streams_t &) = delete;
    void operator=(const io_streams_t &) = delete;

    explicit io_streams_t(size_t read_limit) : out(read_limit), err(read_limit), stdin_fd(-1) {}
};


#endif
