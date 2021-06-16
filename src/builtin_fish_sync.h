// Prototypes for executing builtin_cd function.
#ifndef FISH_BUILTIN_FISH_SYNC_H
#define FISH_BUILTIN_FISH_SYNC_H

#include "maybe.h"

class parser_t;
struct io_streams_t;

maybe_t<int> builtin_fish_sync(parser_t &parser, io_streams_t &streams, const wchar_t **argv);
#endif
