# RUN: %fish -f concurrent %s

function echoer
   echo $argv
end

function write_to_temp
  set which $argv[1]
  set where (mktemp -d)
  cd $where
  for i in (seq 5)
    echoer $i > ./{$which}_file.txt
    command echo $1 > ./{$which}_file2.txt
    sleep .1
  end
  count *
end

write_to_temp 1 &
# CHECK: 2
write_to_temp 2 &
# CHECK: 2
write_to_temp 3 &
# CHECK: 2
write_to_temp 4 &
# CHECK: 2
write_to_temp 5 &
# CHECK: 2

wait
