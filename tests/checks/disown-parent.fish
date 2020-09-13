# RUN: env fish_test_helper=%fish_test_helper %fish --features=no-concurrent %s

# Ensure that a job which attempts to disown itself does not explode.
# Here fish_test_helper is the process group leader; we attempt to disown
# its pid within a pipeline containing it.
# This requires non-concurrent mode; in concurrent mode it doesn't get
# its own pgroup.

function disowner
    read -l pid
    echo Disown $pid
    disown $pid
end
$fish_test_helper print_pid_then_sleep | disowner

# CHECK: Disown {{\d+}}
