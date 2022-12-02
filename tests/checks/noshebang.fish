# RUN: %fish %s

# Test for shebangless scripts - see 7802.

set testdir (mktemp -d)
cd $testdir

# Never implicitly pass files ending with .fish to /bin/sh.
true >file.fish
sleep 0.1
chmod a+x file.fish
set -g fish_use_posix_spawn 0
./file.fish
echo $status
set -g fish_use_posix_spawn 1
./file.fish
echo $status
rm file.fish
#CHECK: 126
#CHECKERR: exec: {{.*}}{{.*}}
#CHECKERR: exec: {{.*}}

#CHECK: 126
#CHECKERR: exec: {{.*}}
#CHECKERR: exec: {{.*}}
