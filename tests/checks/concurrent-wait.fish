# RUN: %fish -f concurrent %s

function sleeper
 sleep 1
end

sleeper &
jobs
# CHECK: Job	Group{{(\tCPU)?}}	State	Command
# CHECK: 1	{{\d+}}{{(\t\d+%)?}}	running	sleeper &

wait

jobs
# CHECK: jobs: There are no jobs
