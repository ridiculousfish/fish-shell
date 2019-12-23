# RUN: %fish -f concurrent %s

function sleeper
 sleep 1
end

sleeper &
jobs
# CHECK: Job	Group	State	Command
# CHECK: 1	-2	running	sleeper &

wait

jobs
# CHECK: jobs: There are no jobs
