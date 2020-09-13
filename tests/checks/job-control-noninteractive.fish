#RUN: env fth=%fish_test_helper fish=%fish %fish %s

# Ensure job control works in non-interactive environments.

status job-control full
/bin/echo hello
#CHECK: hello

set tmppath (mktemp)
$fth print_pgrp > $tmppath
$fth print_pgrp >> $tmppath
read --line first second < $tmppath
test $first -ne $second
and echo "pgroups differed, meaning job control worked"
or echo "pgroups were the same, job control did not work"
#CHECK: pgroups differed, meaning job control worked
rm $tmppath

# fish ignores SIGTTIN and so may transfer the tty even if it
# doesn't own the tty. Ensure that doesn't happen.
$fish -c 'status job-control full ; $fth report_foreground' &
wait
#CHECKERR: background
