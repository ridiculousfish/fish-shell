#!/bin/bash

# This is intended to be run inside the vagrant Docker image,
# from vagrant_launch_docker_ci.sh.

usage() {
    cat << EOF
Usage: $(basename $0) fish_source_dir boxname
EOF
    exit 1
}

# Exit on failure.
set -e

export VAGRANT_FISH_SRC_DIR=$1
export VAGRANT_BOXNAME=$2
export VAGRANT_CPUCOUNT=$(nproc)
test -n "$VAGRANT_FISH_SRC_DIR" || usage
test -n "$VAGRANT_BOXNAME" || usage

cd $(mktemp -d)
echo $"Building in $(pwd)"
envsubst < "${VAGRANT_FISH_SRC_DIR}/vagrant/Vagrantfile.base" > ./Vagrantfile

# Unfortunately multi-line commands appear to fail with vagrant ssh.
BUILD_CMD="uname -a; set -e; set -x; mkdir build && cd build; cmake -DCMAKE_BUILD_TYPE=Debug /fish-source && make -j $(nproc) && make test"

vagrant up
RES=0
vagrant ssh -c "${BUILD_CMD}" || RES=$?
vagrant destroy -f
rm -Rf $PWD
exit $RES
