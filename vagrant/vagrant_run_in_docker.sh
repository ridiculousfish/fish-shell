#!/bin/bash

# This is intended to be run from the host.
# It kicks off a Docker image.

usage() {
    cat <<EOF
Usage: $(basename $0) boxname
EOF
    exit 1
}

# Exit on failure.
set -e
set -x


BOXNAME=$1
test -n "$BOXNAME" || usage

DOCKER_EXTRA_ARGS=""

# Get fish source directory.
FISH_SRC_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. >/dev/null && pwd)

DOCKER_REGISTRY=${DOCKER_REGISTRY:-ghcr.io/fish-shell/fish-ci}
DOCKER_IMAGE=${DOCKER_REGISTRY}/${DOCKER_IMAGE_NAME:-vagrant-runner}

echo "Using Docker image: $DOCKER_IMAGE"

# Use -it if we're in a TTY.
if [ -t 0 ]; then
    DOCKER_EXTRA_ARGS="$DOCKER_EXTRA_ARGS -it"
fi


RES=0
docker run \
    --rm \
    --security-opt seccomp=unconfined \
    --mount type=bind,source="$FISH_SRC_DIR",target=/fish-source,readonly \
    --device /dev/vboxdrv:/dev/vboxdrv \
    $DOCKER_EXTRA_ARGS \
    "$DOCKER_IMAGE" \
    /fish-source/vagrant/vagrant_run_tests_ci.sh /fish-source "$BOXNAME" \
    || RES=$?

exit $RES
