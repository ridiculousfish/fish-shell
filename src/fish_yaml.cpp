#include "config.h"

#include "fish_yaml.h"
#include "yaml.h"

// All of our yaml usage prefers implicit structures.
static constexpr int implicit = 1;

struct fish_yaml_generator_t::impl_t {
    yaml_emitter_t emitter;
    yaml_event_t event;
};

fish_yaml_generator_t::fish_yaml_generator_t(std::string &output)
    : impl_(new impl_t()), output_(output) {
    success_ = yaml_emitter_initialize(&impl_->emitter);
    yaml_emitter_set_output(&impl_->emitter, append_handler, this);
    check_emit(yaml_stream_start_event_initialize(&impl_->event, YAML_UTF8_ENCODING));
    check_emit(
        yaml_document_start_event_initialize(&impl_->event, nullptr, nullptr, nullptr, implicit));
}

inline void fish_yaml_generator_t::check_emit(int success) {
    if (!success) {
        success_ = false;
    }
    if (success_) {
        success_ = yaml_emitter_emit(&impl_->emitter, &impl_->event);
    }
}

bool fish_yaml_generator_t::close() {
    if (!closed_) {
        check_emit(yaml_document_end_event_initialize(&impl_->event, implicit));
        check_emit(yaml_stream_end_event_initialize(&impl_->event));
        yaml_emitter_delete(&impl_->emitter);
        closed_ = true;
    }
    return success_;
}

fish_yaml_generator_t::~fish_yaml_generator_t() { close(); }

void fish_yaml_generator_t::start_mapping() {
    if (!success_) return;
    check_emit(yaml_mapping_start_event_initialize(&impl_->event, nullptr /* anchor */,
                                                   (yaml_char_t *)YAML_MAP_TAG, implicit,
                                                   YAML_BLOCK_MAPPING_STYLE));
}

void fish_yaml_generator_t::end_mapping() {
    if (!success_) return;
    check_emit(yaml_mapping_end_event_initialize(&impl_->event));
}

void fish_yaml_generator_t::start_sequence() {
    if (!success_) return;
    check_emit(yaml_sequence_start_event_initialize(&impl_->event, nullptr /* anchor */,
                                                    (yaml_char_t *)YAML_SEQ_TAG, implicit,
                                                    YAML_BLOCK_SEQUENCE_STYLE));
}

void fish_yaml_generator_t::end_sequence() {
    if (!success_) return;
    check_emit(yaml_sequence_end_event_initialize(&impl_->event));
}

void fish_yaml_generator_t::string_internal(const char *str, size_t len) {
    if (!success_) return;
    int plain_implicit = 1;
    int quoted_implicit = 0;
    check_emit(yaml_scalar_event_initialize(
        &impl_->event, nullptr /* anchor */, (yaml_char_t *)YAML_STR_TAG, (yaml_char_t *)str, len,
        plain_implicit, quoted_implicit, YAML_PLAIN_SCALAR_STYLE));
}

int fish_yaml_generator_t::append_handler(void *data, unsigned char *buffer, size_t size) {
    fish_yaml_generator_t *self = static_cast<fish_yaml_generator_t *>(data);
    self->output_.insert(self->output_.end(), buffer, buffer + size);
    return 1; /* success */
}

struct fish_yaml_reader_t::impl_t {
    yaml_parser_t parser;
    yaml_event_t event;
};

fish_yaml_reader_t::fish_yaml_reader_t(const char *data, size_t size) : impl_(new impl_t()) {
    success_ = yaml_parser_initialize(&impl_->parser);
    if (success_) yaml_parser_set_input_string(&impl_->parser, (const unsigned char *)data, size);
}

static bool populate_read_event(const yaml_event_t &evt, fish_yaml_read_event_t *out) {
    out->position = evt.start_mark.index;
    out->end = evt.end_mark.index;
    out->value.clear();
    switch (evt.type) {
        case YAML_NO_EVENT:
        case YAML_STREAM_START_EVENT:
        case YAML_STREAM_END_EVENT:
        case YAML_DOCUMENT_START_EVENT:
        case YAML_DOCUMENT_END_EVENT:
        case YAML_ALIAS_EVENT:
            // These are ignored.
            return false;

        case YAML_SCALAR_EVENT:
            out->type = fish_yaml_read_event_t::scalar;
            out->value.assign((const char *)evt.data.scalar.value, evt.data.scalar.length);
            return true;

        case YAML_SEQUENCE_START_EVENT:
            out->type = fish_yaml_read_event_t::sequence_start;
            return true;
        case YAML_SEQUENCE_END_EVENT:
            out->type = fish_yaml_read_event_t::sequence_end;
            return true;
        case YAML_MAPPING_START_EVENT:
            out->type = fish_yaml_read_event_t::mapping_start;
            return true;
        case YAML_MAPPING_END_EVENT:
            out->type = fish_yaml_read_event_t::mapping_end;
            return true;
        default:
            assert(0 && "Unknown event type");
            break;
    }
}

bool fish_yaml_reader_t::read_next(fish_yaml_read_event_t *evt) {
    for (;;) {
        if (!(success_ && yaml_parser_parse(&impl_->parser, &impl_->event))) {
            success_ = false;
            return false;
        }
        auto type = impl_->event.type;
        bool populated = populate_read_event(impl_->event, evt);
        yaml_event_delete(&impl_->event);
        if (populated) {
            return true;
        } else if (type == YAML_NO_EVENT) {
            return false;
        }
    }
}

fish_yaml_reader_t::~fish_yaml_reader_t() { yaml_parser_delete(&impl_->parser); }
