FROM docker.io/fedora:36

RUN dnf install -y bluez-mesh
RUN rpm -ql bluez-mesh

RUN mkdir -p /var/lib/meshd/config
RUN mkdir -p /var/lib/meshd/lib

RUN mkdir -p /etc/meshd-init
COPY meshd/config /etc/meshd-init/config
COPY meshd/lib /etc/meshd-init/lib
RUN find /etc/meshd-init

VOLUME /var/lib/meshd/config
VOLUME /var/lib/meshd/lib

ENTRYPOINT [ "/usr/libexec/bluetooth/bluetooth-meshd" ]
