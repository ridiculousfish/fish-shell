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

# Get fish source directory.
FISH_SRC_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. >/dev/null && pwd)

DOCKER_REGISTRY=${DOCKER_REGISTRY:-ghcr.io/fish-shell/fish-ci}
DOCKER_IMAGE=${DOCKER_REGISTRY}/${@:$OPTIND:1}

echo "Using Docker image: $DOCKER_IMAGE"

# Use -it if we're in a TTY.
if [ -t 0 ]; then
    DOCKER_EXTRA_ARGS="$DOCKER_EXTRA_ARGS -it"
fi

# Run tests in it, allowing them to fail without failing this script.
# If we are running docker-in-docker, as we are in CI, then our fish source
# directory will not mount properly in the inner image. So use create/copy/start instead.
CONTAINER_ID=$(
    docker create \
        --rm \
        $DOCKER_EXTRA_ARGS \
        "$DOCKER_IMAGE"
)

docker cp "$FISH_SRC_DIR/." "$CONTAINER_ID":/fish-source/
docker start -a "$CONTAINER_ID"
