#!/usr/bin/env fish

# Finds global variables by parsing the output of 'nm'
# for object files in this directory.
# This was written for macOS nm.

set total_globals 0
set boring_files \
    fish_key_reader.cpp.o \
    fish_tests.cpp.o \
    fish_indent.cpp.o \


set misc_whitelist \
    termsize_lock termsize \
    _debug_level \
    sitm_esc ritm_esc dim_esc \
    iothread_init()::inited \
    s_result_queue s_main_thread_request_queue s_read_pipe s_write_pipe \
    s_main_thread_performer_lock s_main_thread_performer_cond s_main_thread_request_q_lock \
    locked_consumed_job_ids \
    s_profiling_output_filename \
    s_sigchld_generation_cnt \


# Variables that are set once during initialization.
set set_once_whitelist \
    initial_pid initial_fg_process_group \
    var_name_prefix \
    env_initialized \
    main_thread_id \
    thread_asserts_cfg_for_testing \
    var_dispatch_table \
    locale_variables \
    curses_variables \
    can_set_term_title \
    'history_t::save_internal_unless_disabled()::seed' \
    'fish_c_locale()::loc' \
    dflt_pathsv \


# Locks and once_flags
set locks_whitelist \
    iothread_init()::inited \
    env_lock \

# Thread-local variabless
set tld_whitelist \
    tld_transient_stack \
    tld_last_status

# Actually global data.
set known_globals_whitelist \
    s_completion_set \
    histories \
    wgettext_map \

# Misc atomics
set known_atomics_whitelist \
    '(anonymous namespace)::history_file_lock(int, int)::do_locking'

set whitelist \
    $misc_whitelist $set_once_whitelist $locks_whitelist \
    $tld_whitelist $known_globals_whitelist $known_atomics_whitelist

for file in ./**.o
    set filename (basename $file)
    # Skip boring files.
    contains $filename $boring_files
    and continue
    for line in (nm -p -P -U $file)
        # Look in data (dD) and bss (bB) segments.
        set matches (string match --regex '^([^ ]+) ([dDbB])' -- $line)
        or continue
        set symname (echo $matches[2] | c++filt)
        contains $symname $whitelist
        and continue

        # Skip guard variables
        string match -q '*guard variable*' $symname
        and continue

        echo $filename $symname $matches[3]
        set total_globals (math $total_globals + 1)
    end
end

echo "Total: $total_globals"
