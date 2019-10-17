# RUN: %fish -f concurrent %s

# This ensures that multiple concurrent functions can each 
# "cd" to a different directory and will not interefere
# with each other.

function echoer
   echo $argv
end

function write_to_temp
  set which $argv[1]
  cd dir$which
  for i in (seq 5)
    echoer $i > ./{$which}_file.txt
    command echo $1 > ./{$which}_file2.txt
    sleep .1
  end
  set -g COUNT$which (count *)
end

set tmpdir (realpath (mktemp -d))
cd $tmpdir

set subdirs dir(seq 10) # like dir1, dir2...dir5
for dir in $subdirs
    mkdir $dir
end

write_to_temp 1 |
write_to_temp 2 |
write_to_temp 3 |
write_to_temp 4 |
write_to_temp 5 |
write_to_temp 6 |
write_to_temp 7 |
write_to_temp 8 |
write_to_temp 9 |
write_to_temp 10

# The pipeline should not have changed our PWD.
test "$PWD" = "$tmpdir"
and echo "PWD not changed"
or echo "PWD changed"
#CHECK: PWD not changed

# Each directory should have two files.
echo $COUNT1 (count dir1/*)
#CHECK: 2 2
echo $COUNT2 (count dir2/*)
#CHECK: 2 2
echo $COUNT3 (count dir3/*)
#CHECK: 2 2
echo $COUNT4 (count dir4/*)
#CHECK: 2 2
echo $COUNT5 (count dir5/*)
#CHECK: 2 2
echo $COUNT6 (count dir6/*)
#CHECK: 2 2
echo $COUNT7 (count dir7/*)
#CHECK: 2 2
echo $COUNT8 (count dir8/*)
#CHECK: 2 2
echo $COUNT9 (count dir9/*)
#CHECK: 2 2
echo $COUNT10 (count dir10/*)
#CHECK: 2 2
