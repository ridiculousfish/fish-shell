# RUN: %fish %s

# Test quoted cmdsubs

count (seq 3)
# CHECK: 3

count "$(seq 3)"
# CHECK: 1

echo "$(echo a; echo b;)"
# CHECK: a b

echo "$(seq (echo 3))"
# CHECK: 1 2 3

echo "$(echo "$(echo "$(echo "$(echo a)")")")"
# CHECK: a
