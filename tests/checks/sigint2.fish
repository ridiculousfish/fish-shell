#RUN: %fish -C "set helper %fish_test_helper" -f concurrent %s

# Ensure we can break from a multithreaded pipeline.

function forever ; while true ; end; end

echo About to sigint
$helper sigint_parent &
forever | forever | forever
echo I should not be printed because I got sigint

#CHECK: About to sigint
#CHECKERR: Sent SIGINT to {{\d*}}
