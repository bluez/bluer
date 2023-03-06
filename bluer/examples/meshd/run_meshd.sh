#!/bin/bash
rm -rf /tmp/meshd
mkdir /tmp/meshd

BASEDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

cp -r ${BASEDIR}/config /tmp/meshd/config
cp -r ${BASEDIR}/lib /tmp/meshd/lib

find /tmp/meshd

sudo /usr/libexec/bluetooth/bluetooth-meshd --config /tmp/meshd/config --storage /tmp/meshd/lib --nodetach --debug
