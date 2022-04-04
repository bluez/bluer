//! Bluetooth profiles for RFCOMM connections.

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    nonblock::Proxy,
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::Future;
use pin_project::{pin_project, pinned_drop};
use std::{
    collections::HashMap,
    fmt,
    os::unix::io::IntoRawFd,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use strum::{Display, EnumString, IntoStaticStr};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::{Socket, Stream};
use crate::{method_call, read_dict, Address, Device, Result, SessionInner, ERR_PREFIX, SERVICE_NAME, TIMEOUT};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.ProfileManager1";
pub(crate) const MANAGER_PATH: &str = "/org/bluez";
pub(crate) const PROFILE_INTERFACE: &str = "org.bluez.Profile1";
pub(crate) const PROFILE_PREFIX: &str = publish_path!("profile/");

/// Error response from us to a Bluetooth profile request.
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
#[derive(Clone, Copy, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, IntoStaticStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReqError {
    /// Request was rejected.
    Rejected,
    /// Request was canceled.
    Canceled,
}

impl std::error::Error for ReqError {}

impl Default for ReqError {
    fn default() -> Self {
        Self::Canceled
    }
}

impl From<ReqError> for dbus::MethodErr {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Result of a Bluetooth profile request to us.
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
pub type ReqResult<T> = std::result::Result<T, ReqError>;

/// Local profile role.
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Display, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Role {
    /// Client.
    #[strum(serialize = "client")]
    Client,
    /// Server.
    #[strum(serialize = "server")]
    Server,
}

/// Bluetooth RFCOMM profile definition.
///
/// Use [Session::register_profile](crate::Session::register_profile) to register a profile.
///
/// Some predefined services:
///
///   * HFP AG UUID: `0000111f-0000-1000-8000-00805f9b34fb`
///     Default profile Version is 1.7, profile Features
///     is 0b001001 and RFCOMM channel is 13.
///     Authentication is required.
///
///   * HFP HS UUID: `0000111e-0000-1000-8000-00805f9b34fb`
///     Default profile Version is 1.7, profile Features
///     is 0b000000 and RFCOMM channel is 7.
///     Authentication is required.
///
///   * HSP AG UUID: `00001112-0000-1000-8000-00805f9b34fb`
///     Default profile Version is 1.2, RFCOMM channel
///     is 12 and Authentication is required. Does not
///     support any Features, option is ignored.
///
///   * HSP HS UUID: `00001108-0000-1000-8000-00805f9b34fb`
///     Default profile Version is 1.2, profile features
///     is 0b0 and RFCOMM channel is 6. Authentication
///     is required. Features is one bit value, specify
///     capability of Remote Audio Volume Control
///     (by default turned off).
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
#[derive(Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Profile {
    /// Profile UUID.
    pub uuid: Uuid,
    /// Human readable name for the profile.
    pub name: Option<String>,
    /// The primary service class UUID (if different from the actual profile UUID).
    pub service: Option<Uuid>,
    /// For asymmetric profiles that do not have UUIDs available to uniquely identify
    /// each side this parameter allows specifying the precise local role.
    pub role: Option<Role>,
    /// RFCOMM channel number that is used for client and server UUIDs.
    ///
    /// If applicable it will be used in the SDP record as well.
    pub channel: Option<u16>,
    /// PSM number that is used for client and server UUIDs.
    ///
    /// If applicable it will be used in the SDP record as well.
    pub psm: Option<u16>,
    /// Pairing is required before connections will be established.
    /// No devices will be connected if not paired.
    pub require_authentication: Option<bool>,
    /// Request authorization before any connection will be established.
    pub require_authorization: Option<bool>,
    /// In case of a client UUID this will force connection of the RFCOMM or L2CAP
    /// channels when a remote device is connected.
    pub auto_connect: Option<bool>,
    /// Provide a manual SDP record.
    pub service_record: Option<String>,
    /// Profile version (for SDP record).
    pub version: Option<u16>,
    /// Profile features (for SDP record).
    pub features: Option<u16>,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Profile {
    fn to_dict(&self) -> PropMap {
        let mut pm = PropMap::new();
        if let Some(name) = &self.name {
            pm.insert("Name".to_string(), Variant(name.box_clone()));
        }
        if let Some(service) = &self.service {
            pm.insert("Service".to_string(), Variant(service.to_string().box_clone()));
        }
        if let Some(role) = &self.role {
            pm.insert("Role".to_string(), Variant(role.to_string().box_clone()));
        }
        if let Some(channel) = &self.channel {
            pm.insert("Channel".to_string(), Variant(channel.box_clone()));
        }
        if let Some(psm) = &self.psm {
            pm.insert("PSM".to_string(), Variant(psm.box_clone()));
        }
        if let Some(require_authentication) = &self.require_authentication {
            pm.insert("RequireAuthentication".to_string(), Variant(require_authentication.box_clone()));
        }
        if let Some(require_authorization) = &self.require_authorization {
            pm.insert("RequireAuthorization".to_string(), Variant(require_authorization.box_clone()));
        }
        if let Some(auto_connect) = &self.auto_connect {
            pm.insert("AutoConnect".to_string(), Variant(auto_connect.box_clone()));
        }
        if let Some(service_record) = &self.service_record {
            pm.insert("ServiceRecord".to_string(), Variant(service_record.box_clone()));
        }
        if let Some(version) = &self.version {
            pm.insert("Version".to_string(), Variant(version.box_clone()));
        }
        if let Some(features) = &self.features {
            pm.insert("Features".to_string(), Variant(features.box_clone()));
        }
        pm
    }
}

/// A request to connect to this profile, either as client or server.
///
/// The new service level connection has been made and authorized.
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
pub struct ConnectRequest {
    device: Address,
    fd: OwnedFd,
    props: ConnectRequestProps,
    tx: oneshot::Sender<ReqResult<()>>,
    closed_tx: mpsc::Sender<()>,
}

impl fmt::Debug for ConnectRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ConnectRequest")
            .field("device", &self.device)
            .field("version", &self.version())
            .field("features", &self.features())
            .finish_non_exhaustive()
    }
}

impl ConnectRequest {
    /// Device address.
    pub fn device(&self) -> Address {
        self.device
    }

    /// Profile version.
    pub fn version(&self) -> Option<u16> {
        self.props.version
    }

    /// Profile features.
    pub fn features(&self) -> Option<u16> {
        self.props.features
    }

    /// Returns a future that resolves when the profile gets disconnected.
    ///
    /// The file descriptor is no longer owned by the service
    /// daemon and the profile implementation needs to take
    /// care of cleaning up all connections.
    pub fn closed(&self) -> impl Future<Output = ()> {
        let closed_tx = self.closed_tx.clone();
        async move { closed_tx.closed().await }
    }

    /// Accept the connection request and establish an RFCOMM connection.
    pub fn accept(self) -> Result<Stream> {
        let Self { fd, tx, .. } = self;

        let socket = unsafe { Socket::from_raw_fd(fd.into_raw_fd()) }?;
        let stream = Stream::from_socket(socket)?;
        let _ = tx.send(Ok(()));

        Ok(stream)
    }

    /// Reject the connection request.
    pub fn reject(self, reason: ReqError) {
        let _ = self.tx.send(Err(reason));
    }
}

#[derive(Clone, Debug)]
struct ConnectRequestProps {
    pub version: Option<u16>,
    pub features: Option<u16>,
}

impl ConnectRequestProps {
    fn from_dict(dict: &HashMap<String, Variant<Box<dyn RefArg + 'static>>>) -> Self {
        Self {
            version: read_dict(dict, "Version").ok().cloned(),
            features: read_dict(dict, "Features").ok().cloned(),
        }
    }
}

pub(crate) struct RegisteredProfile {
    req_tx: mpsc::Sender<ConnectRequest>,
    device_closed_rx: Mutex<HashMap<Address, Vec<mpsc::Receiver<()>>>>,
}

impl RegisteredProfile {
    pub(crate) fn new(req_tx: mpsc::Sender<ConnectRequest>) -> Self {
        Self { req_tx, device_closed_rx: Mutex::new(HashMap::new()) }
    }

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(PROFILE_INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async(
                "NewConnection",
                ("device", "fd", "fd_properties"),
                (),
                |ctx, cr, (device_path, fd, props): (dbus::Path<'static>, OwnedFd, PropMap)| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        let device = if let Some((_, device)) = Device::parse_dbus_path(&device_path) {
                            device
                        } else {
                            log::error!("Cannot parse device path: {}", &device_path);
                            return Err(ReqError::Rejected.into());
                        };
                        let props = ConnectRequestProps::from_dict(&props);

                        let (tx, rx) = oneshot::channel();
                        let (closed_tx, closed_rx) = mpsc::channel(1);

                        let cr = ConnectRequest { device, fd, props, tx, closed_tx };
                        let _ = reg.req_tx.send(cr).await;

                        match rx.await {
                            Ok(Ok(())) => {
                                let mut device_closed_rx = reg.device_closed_rx.lock().await;
                                device_closed_rx.entry(device).or_default().push(closed_rx);
                                Ok(())
                            }
                            Ok(Err(err)) => Err(err.into()),
                            Err(_) => Err(ReqError::Rejected.into()),
                        }
                    })
                },
            );

            ib.method_with_cr_async(
                "RequestDisconnection",
                ("device",),
                (),
                |ctx, cr, (device_path,): (dbus::Path<'static>,)| {
                    method_call(ctx, cr, |reg: Arc<Self>| async move {
                        let device = if let Some((_, device)) = Device::parse_dbus_path(&device_path) {
                            device
                        } else {
                            log::error!("Cannot parse device path: {}", &device_path);
                            return Err(ReqError::Rejected.into());
                        };

                        let mut device_closed_rx = reg.device_closed_rx.lock().await;
                        device_closed_rx.remove(&device);
                        Ok(())
                    })
                },
            );
        })
    }

    pub(crate) async fn register(
        self, inner: Arc<SessionInner>, profile: Profile, req_rx: mpsc::Receiver<ConnectRequest>,
    ) -> Result<ProfileHandle> {
        let name = dbus::Path::new(format!("{}{}", PROFILE_PREFIX, Uuid::new_v4().to_simple())).unwrap();
        log::trace!("Publishing profile at {}", &name);

        {
            let mut cr = inner.crossroads.lock().await;
            cr.insert(name.clone(), &[inner.profile_token], Arc::new(self));
        }

        log::trace!("Registering profile at {}", &name);
        let proxy = Proxy::new(SERVICE_NAME, MANAGER_PATH, TIMEOUT, inner.connection.clone());
        proxy
            .method_call(
                MANAGER_INTERFACE,
                "RegisterProfile",
                (name.clone(), profile.uuid.to_string(), profile.to_dict()),
            )
            .await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_name = name.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering profile at {}", &unreg_name);
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterProfile", (unreg_name.clone(),)).await;

            log::trace!("Unpublishing profile at {}", &unreg_name);
            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&unreg_name);
        });

        Ok(ProfileHandle { name, req_rx: ReceiverStream::new(req_rx), _drop_tx: drop_tx })
    }
}

/// Handle to registered Bluetooth RFCOMM profile receiving its connect requests.
///
/// Drop to unregister profile.
#[cfg_attr(docsrs, doc(cfg(all(feature = "rfcomm", feature = "bluetoothd"))))]
#[pin_project(PinnedDrop)]
pub struct ProfileHandle {
    name: dbus::Path<'static>,
    #[pin]
    req_rx: ReceiverStream<ConnectRequest>,
    _drop_tx: oneshot::Sender<()>,
}

impl futures::stream::Stream for ProfileHandle {
    type Item = ConnectRequest;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().req_rx.poll_next(cx)
    }
}

#[pinned_drop]
impl PinnedDrop for ProfileHandle {
    fn drop(self: Pin<&mut Self>) {
        // required for drop order
    }
}

impl fmt::Debug for ProfileHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProfileHandle {{ {} }}", &self.name)
    }
}
