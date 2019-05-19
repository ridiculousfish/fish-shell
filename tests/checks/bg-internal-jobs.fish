# RUN: %fish -f concurrent %s

# Supress fg setting the term title
set -x TERM dumb

function sleeper
    sleep .5
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
