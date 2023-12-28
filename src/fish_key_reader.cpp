// A small utility to print information related to pressing keys. This is similar to using tools
// like `xxd` and `od -tx1z` but provides more information such as the time delay between each
// character. It also allows pressing and interpreting keys that are normally special such as
// [ctrl-C] (interrupt the program) or [ctrl-D] (EOF to signal the program should exit).
// And unlike those other tools this one disables ICRNL mode so it can distinguish between
// carriage-return (\cM) and newline (\cJ).
//
// Type "exit" or "quit" to terminate the program.
#include "config.h"  // IWYU pragma: keep

#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <termios.h>
#include <unistd.h>

#include <cstring>
#include <cwchar>
#include <string>
#include <vector>

#include "common.h"
#include "cxxgen.h"
#include "env.h"
#include "env/env_ffi.rs.h"
#include "fallback.h"  // IWYU pragma: keep
#include "ffi_baggage.h"
#include "ffi_init.rs.h"
#include "fish_key_reader.rs.h"
#include "fish_version.h"
#include "input_ffi.rs.h"
#include "maybe.h"
#include "parser.h"
#include "print_help.rs.h"
#include "proc.h"
#include "reader.h"
#include "signals.h"
#include "wutil.h"  // IWYU pragma: keep

int main() { fish_key_reader_main(); }
