#!/usr/bin/env bash

set -ex

REF=$1
VERSION=${REF##refs/tags/}
TARGET=$2

td=$(mktemp -d)
mkdir dist
out_dir=$(pwd)/dist
name="${PROJECT_NAME}-${VERSION}-${TARGET}"

cp target/release/pack "$td/"
cp README.md "$td/"

pushd $td
tar czf "$out_dir/$name.tar.gz" *
popd
rm -r $td

echo ::set-output name=name::$name.tar.gz
