# RUN: env fth=%fish_test_helper %fish %s

status job-control full

# Ensure that lots of nested jobs all end up in the same pgroup.

function save_pgroup -a var_name
    $fth print_pgrp | read -g $var_name
end

# Here everything should live in the pgroup of the first fish_test_helper.
$fth print_pgrp | read -g global_group | save_pgroup g1 | begin
    save_pgroup g2
end | begin
    echo (save_pgroup g3) >/dev/null
end

[ "$global_group" -eq "$g1" ] && [ "$g1" -eq "$g2" ] && [ "$g2" -eq "$g3" ]
and echo "All pgroups agreed"
or echo "Pgroups disagreed. Should be in $global_group but found $g1, $g2, $g3"
# CHECK: All pgroups agreed

### DISABLED - this fails a lot on Travis
# Ensure that eval retains pgroups - #6806.
# Our regex will capture the first pgroup and use a positive lookahead on the second.
# $fth print_pgrp | tr \n ' ' 1>&2 | eval '$fth print_pgrp' 1>&2
## CHECKERR: {{(\d+) (?=\1)\d+}}

# Ensure that if a background job launches another background job, that they have different pgroups.
# The pipeline here will arrange for the two pgroups to be printed on the same line, like:
#   123 124
# Our regex will capture the first pgroup and use a negative lookahead on the second.
status job-control full
$fth print_pgrp | begin
    tr \n ' '
    $fth print_pgrp | tr \n ' ' &
end &
wait
echo
# CHECK: {{(\d+) (?!\1)\d+}}
