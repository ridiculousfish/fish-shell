# RUN: %fish -f concurrent -C "set helper %fish_test_helper" %s

# Supress fg setting the term title
set -x TERM dumb

function sleeper
    sleep .5
end

function forever
    while true
    end
end

status job-control full
sleeper &
status job-control interactive

jobs
# CHECK: Job	Group	State	Command
# CHECK: 1	-2	running	sleeper &

fg
# CHECKERR: Send job 1, 'sleeper &' to foreground

jobs
# CHECK: jobs: There are no jobs

$helper sigint_parent
forever | forever | forever
# CHECK: Sent SIGINT to {{\d+}}
jobs
# CHECK: jobs: There are no jobs

$helper sigstop_parent
forever | forever | forever
# CHECK: Sent SIGSTOP to {{\d+}}
