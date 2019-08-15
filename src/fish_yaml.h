#ifndef FISH_YAML_H
#define FISH_YAML_H

#include <memory>
#include <string>

/// A class that wraps libyaml, allowing for generating yaml.
/// Note this is a leaf library of the fish shell. Do not introduce new fish shell dependencies in
/// here.
class fish_yaml_generator_t {
   public:
    /// Construct a fish_yaml_generator_t, generating text appending to a given string \p output.
    fish_yaml_generator_t(std::string &output);

    /// Closes the generator, flushing everything to the given output string. This is idempotent and
    /// called automatically in the destructor. \return true on success, false if an error has
    /// occurred.
    bool close();

    ~fish_yaml_generator_t();

    void start_mapping();
    void end_mapping();

    void start_sequence();
    void end_sequence();

    void string(const char *str) { string_internal(str, strlen(str)); }

    void string(const std::string &str) { string_internal(str.c_str(), str.size()); }

    void key_value(const char *key, const char *value) {
        string(key);
        string(value);
    }

   private:
    void string_internal(const char *str, size_t len);

    static int append_handler(void *data, unsigned char *buffer, size_t size);

    inline void check_emit(int success);

    struct impl_t;
    std::unique_ptr<impl_t> impl_;
    std::string &output_;
    int success_{true};
    bool closed_{false};
};

struct fish_yaml_read_event_t {
    enum type_t {
        /// The stream is complete.
        stream_end,

        // Opening and closing an object.
        mapping_start,
        mapping_end,

        // Opening and closing a sequence.
        sequence_start,
        sequence_end,

        /// A scalar type.
        scalar,
    };

    /// The type of the event.
    type_t type{stream_end};

    /// For scalar events, the contents.
    std::string value{};

    /// The position of the event.
    size_t position{size_t(-1)};

    fish_yaml_read_event_t() = default;
};

class fish_yaml_reader_t {
   public:
    fish_yaml_reader_t(const char *data, size_t size);
    ~fish_yaml_reader_t();

    bool read_next(fish_yaml_read_event_t *evt);

   private:
    struct impl_t;
    std::unique_ptr<impl_t> impl_;
    int success_{true};
};

#endif
