#!/bin/bash

usage() {
    cat <<EOF
Usage: $(basename $0) DOCKER-IMAGE-TAG
EOF
    exit 1
}

DOCKER_EXTRA_ARGS=""

# Exit on failure.
set -e
set -x

# Get fish source directory.
FISH_SRC_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. >/dev/null && pwd)

DOCKER_IMAGE_NAME=$1
test -n "$DOCKER_IMAGE_NAME" || usage

DOCKER_IMAGE=ghcr.io/fish-shell/fish-ci/${DOCKER_IMAGE_NAME}

echo "Using Docker image: $DOCKER_IMAGE"

# Use -it if we're in a TTY.
if [ -t 0 ]; then
    DOCKER_EXTRA_ARGS="$DOCKER_EXTRA_ARGS -it"
fi

# Run tests in it, allowing them to fail without failing this script.
# If we are running docker-in-docker, as we are in CI, then our fish source
# directory will not mount properly in the inner image. So use run with sleep
# instead, then later exec.
# We need seccomp=unconfined otherwise posix_spawn barfs on fedora.
CONTAINER_ID=$(
    docker run \
        --detach \
        --rm \
        --security-opt seccomp=unconfined \
        $DOCKER_EXTRA_ARGS \
        "$DOCKER_IMAGE" \
        sleep infinity
)

docker cp "$FISH_SRC_DIR/." "$CONTAINER_ID":/fish-source/
docker exec --user root "$CONTAINER_ID" chown -R fishuser /fish-source
docker exec --user fishuser "$CONTAINER_ID" /fish-source/docker/context/fish_run_tests.sh
