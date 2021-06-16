// Functions for syncing fish universal config.
#include "config.h"  // IWYU pragma: keep

#include "builtin_fish_sync.h"

#include "builtin.h"
#include "common.h"
#include "env_universal_common.h"
#include "io.h"
#include "parser.h"
#include "wgetopt.h"

/// Implementation of fish_sync builtin.
maybe_t<int> builtin_fish_sync(parser_t &parser, io_streams_t &streams, const wchar_t **argv) {
    const wchar_t *cmd = argv[0];
    int argc = builtin_count_args(argv);

    static const wchar_t *const short_options = L"v:h";
    static const struct woption long_options[] = {{L"var", required_argument, nullptr, L'v'},
                                                  {L"help", no_argument, nullptr, L'h'},
                                                  {nullptr, 0, nullptr, 0}};

    wcstring_list_t var_names;

    bool print_help = false;

    int opt;
    wgetopter_t w;
    while ((opt = w.wgetopt_long(argc, argv, short_options, long_options, nullptr)) != -1) {
        switch (opt) {
            case L'v': {
                var_names.push_back(w.woptarg);
                break;
            }
            case L':': {
                builtin_missing_argument(parser, streams, cmd, argv[w.woptind - 1]);
                return STATUS_INVALID_ARGS;
            }
            case L'h': {
                print_help = true;
                break;
            }
            case L'?': {
                builtin_unknown_option(parser, streams, cmd, argv[w.woptind - 1]);
                return STATUS_INVALID_ARGS;
            }
        }
    }

    if (print_help) {
        builtin_print_help(parser, streams, argv[0]);
        return STATUS_CMD_OK;
    }

    if (w.woptind != argc) {
        streams.err.append_format(BUILTIN_ERR_TOO_MANY_ARGUMENTS, cmd);
        return STATUS_INVALID_ARGS;
    }

    bool success = true;
    bool needs_rerun = false;
    config_universal_t &uconf = config_universal_t::shared();
    if (var_names.empty()) {
        // Nothing new to write, just run the config if changed.
        needs_rerun = uconf.check_file_changed();
    } else {
        // We have some new variables to write.
        success = uconf.update(var_names, parser.context(), &needs_rerun);
        if (success) universal_notifier_t::default_notifier().post_notification();
    }
    if (needs_rerun) uconf.run_config(parser);

    return success ? STATUS_CMD_OK : STATUS_CMD_ERROR;
}
