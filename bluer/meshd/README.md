# meshcfg

Running a mesh network on a Linux host with bluetooth mesh daemon.

To start the daemon:

```
mkdir -p ${PWD}/lib
sudo /usr/libexec/bluetooth/bluetooth-meshd --config ${PWD}/config --storage ${PWD}/lib --debug
```

## Building new images

Run the following commands from the directory this file is located in:

```shell
podman build  ../.. -f infra/meshd/Dockerfile -t quay.io/eclipsecon-2022/meshd:latest
podman push quay.io/eclipsecon-2022/meshd:latest
```
