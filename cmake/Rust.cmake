include(FetchContent)

set(FISH_HEADER_OUTDIR "${CMAKE_CURRENT_BINARY_DIR}/fishrust-headers")

FetchContent_Declare(
    Corrosion
    GIT_REPOSITORY https://github.com/ridiculousfish/corrosion
    GIT_TAG fish-experiments
)

FetchContent_MakeAvailable(Corrosion)

corrosion_import_crate(
    MANIFEST_PATH "${CMAKE_SOURCE_DIR}/fishcrate/Cargo.toml"
)
set(FISHRUST_TARGET "fish-rust")
corrosion_set_hostbuild(${FISHRUST_TARGET})
corrosion_set_env_vars(${FISHRUST_TARGET} "FISH_SRC_DIR=${CMAKE_SOURCE_DIR}/src")
corrosion_set_env_vars(${FISHRUST_TARGET} "FISH_BUILD_DIR=${CMAKE_BINARY_DIR}")
corrosion_set_env_vars(${FISHRUST_TARGET} "FISH_HEADER_OUTDIR=${FISH_HEADER_OUTDIR}")

target_include_directories(${FISHRUST_TARGET} INTERFACE "${FISH_HEADER_OUTDIR}/include")
