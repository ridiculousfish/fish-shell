# RUN: env fth=%fish_test_helper %fish -f concurrent %s

# Ensure that lots of nested jobs all end up in the same pgroup.
status job-control full

function save_pgroup -a var_name
    $fth print_pgrp | read -g $var_name
end

save_pgroup g1 | save_pgroup g2
[ "$g1" -eq "$g2" ]
and echo "All pgroups agreed"
or echo "Pgroups disagreed. Found $g1 and $g2"
# CHECK: All pgroups agreed
