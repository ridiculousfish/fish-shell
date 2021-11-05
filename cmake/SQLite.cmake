find_package(SQLite3)

include_directories(${SQLite3_INCLUDE_DIRS})

# Define sha3 library.
set(SHA3_LIB fish_sha3)
add_library(${SHA3_LIB} STATIC sha3/sha3.c)
target_include_directories(${SHA3_LIB} PUBLIC sha3)
