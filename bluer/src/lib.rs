#![cfg_attr(docsrs, feature(doc_cfg))]

//! # BlueR — Official BlueZ Bindings for Rust
//!
//! This library provides the official Rust interface to the [Linux Bluetooth protocol stack (BlueZ)].
//! Both publishing local and consuming remote [GATT services] using *idiomatic* Rust code is supported.
//! L2CAP and RFCOMM sockets are presented using an API similar to Tokio networking.
//!
//! This library depends on the [tokio] asynchronous runtime.
//!
//! The following functionality is provided.
//!
//! * [Bluetooth adapters](Adapter)
//!     * [enumeration](Session::adapter_names)
//!     * configuration of power, discoverability, name, etc.
//!     * hot-plug support through change events stream
//! * [Bluetooth devices](Device)
//!     * [discovery](Adapter::discover_devices)
//!     * querying of address, name, class, signal strength (RSSI), etc.
//!     * Bluetooth Low Energy advertisements
//!     * [change events stream](Adapter::events)
//!     * connecting and pairing
//! * [consumption of remote GATT services](Device::services)
//!     * GATT service discovery
//!     * read, write and notify operations on characteristics
//!     * read and write operations on characteristic descriptors
//!     * optional use of low-overhead [AsyncRead] and [AsyncWrite] streams for notify and write operations
//! * [publishing local GATT services](Adapter::serve_gatt_application)
//!     * read, write and notify operations on characteristics
//!     * read and write operations on characteristic descriptors
//!     * two programming models supported
//!         * callback-based interface
//!         * low-overhead [AsyncRead] and [AsyncWrite] streams
//! * [sending Bluetooth Low Energy advertisements](Adapter::advertise)
//! * [Bluetooth authorization agent](agent::Agent)
//! * efficient event dispatching
//!     * not affected by D-Bus match rule count
//!     * O(1) in number of subscriptions
//! * [L2CAP sockets](l2cap)
//!     * support for both classic Bluetooth (BR/EDR) and Bluetooth LE
//!     * stream oriented
//!     * sequential packet oriented
//!     * datagram oriented
//!     * async IO interface with [AsyncRead] and [AsyncWrite] support
//! * [RFCOMM sockets](rfcomm)
//!     * support for classic Bluetooth (BR/EDR)
//!     * stream oriented
//!     * async IO interface with [AsyncRead] and [AsyncWrite] support
//! * [database of assigned numbers](id)
//!     * manufacturer ids
//!     * services classes, GATT services, characteristics and descriptors
//!
//! Currently, some classic Bluetooth (BR/EDR) functionality is missing.
//! However, pull requests and contributions are welcome!
//!
//! ## Crate features
//! The following crate features are available.
//!
//! * `bluetoothd`: Enables all functions requiring a running Bluetooth daemon.
//! * `id`: Enables database of assigned numbers.
//! * `l2cap`: Enables L2CAP sockets.
//! * `rfcomm`: Enables RFCOMM sockets.
//! * `serde`: Enables serialization and deserialization of some data types.
//!
//! To enable all crate features specify the `full` crate feature.
//!
//! ## Basic usage
//! Create a [Session] using [Session::new]; this establishes a connection to the Bluetooth daemon.
//! Then obtain a Bluetooth adapter using [Session::adapter].
//! From there on you can access most of the functionality using the methods provided by [Adapter].
//!
//! ## L2CAP and RFCOMM sockets
//! Refer to the [l2cap] and [rfcomm] modules.
//! No [Session] and therefore no running Bluetooth daemon is required.
//!
//! [Linux Bluetooth protocol stack (BlueZ)]: http://www.bluez.org/
//! [GATT services]: https://www.oreilly.com/library/view/getting-started-with/9781491900550/ch04.html
//! [AsyncRead]: tokio::io::AsyncRead
//! [AsyncWrite]: tokio::io::AsyncWrite

#![warn(missing_docs)]

#[cfg(not(target_os = "linux"))]
compile_error!("BlueR only supports the Linux operating system.");

#[cfg(feature = "bluetoothd")]
use dbus::{
    arg::{prop_cast, AppendAll, PropMap, RefArg, Variant},
    nonblock::{stdintf::org_freedesktop_dbus::ObjectManager, Proxy, SyncConnection},
    Path,
};
#[cfg(feature = "bluetoothd")]
use dbus_crossroads::{Context, Crossroads};
#[cfg(feature = "bluetoothd")]
use futures::Future;
#[cfg(feature = "bluetoothd")]
use hex::FromHex;
use macaddr::MacAddr6;
use num_derive::FromPrimitive;
#[cfg(feature = "bluetoothd")]
use std::{collections::HashMap, marker::PhantomData, sync::Arc, time::Duration};
use std::{
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    ops::{Deref, DerefMut},
    str::FromStr,
};
use strum::{Display, EnumString};
#[cfg(feature = "bluetoothd")]
use tokio::task::JoinError;

#[cfg(feature = "bluetoothd")]
pub(crate) const SERVICE_NAME: &str = "org.bluez";
#[cfg(feature = "bluetoothd")]
pub(crate) const ERR_PREFIX: &str = "org.bluez.Error.";
#[cfg(feature = "bluetoothd")]
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(feature = "bluetoothd")]
macro_rules! publish_path {
    ($path:expr) => {
        concat!("/org/bluez/", env!("CARGO_PKG_NAME"), "/", $path)
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! dbus_interface {
    () => {
        #[allow(dead_code)]
        async fn get_property_with_interface<R>(&self, name: &str, interface: &str) -> crate::Result<R>
        where
            R: for<'b> dbus::arg::Get<'b> + std::fmt::Debug + 'static,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            let value = self.proxy().get(interface, name).await?;
            log::trace!("{}: {}.{} = {:?}", &self.proxy().path, &interface, &name, &value);
            Ok(value)
        }

        #[allow(dead_code)]
        async fn get_opt_property_with_interface<R>(
            &self, name: &str, interface: &str,
        ) -> crate::Result<Option<R>>
        where
            R: for<'b> dbus::arg::Get<'b> + std::fmt::Debug + 'static,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            match self.proxy().get(interface, name).await {
                Ok(value) => {
                    log::trace!("{}: {}.{} = {:?}", &self.proxy().path, &interface, &name, &value);
                    Ok(Some(value))
                }
                Err(err) if err.name() == Some("org.freedesktop.DBus.Error.InvalidArgs") => {
                    log::trace!("{}: {}.{} = None", &self.proxy().path, &interface, &name);
                    Ok(None)
                }
                Err(err) => Err(err.into()),
            }
        }

        #[allow(dead_code)]
        async fn set_property_with_interface<T>(&self, name: &str, value: T, interface: &str) -> crate::Result<()>
        where
            T: dbus::arg::Arg + dbus::arg::Append + std::fmt::Debug,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            log::trace!("{}: {}.{} := {:?}", &self.proxy().path, &interface, &name, &value);
            self.proxy().set(interface, name, value).await?;
            Ok(())
        }

        #[allow(dead_code)]
        async fn call_method_with_interface<A, R>(&self, name: &str, args: A, interface: &str) -> crate::Result<R>
        where
            A: dbus::arg::AppendAll + std::fmt::Debug,
            R: dbus::arg::ReadAll + std::fmt::Debug + 'static,
        {
            log::trace!("{}: {}.{} {:?}", &self.proxy().path, &interface, &name, &args);
            let result = self.proxy().method_call(interface, name, args).await;
            log::trace!("{}: {}.{} (...) -> {:?}", &self.proxy().path, &interface, &name, &result);
            Ok(result?)
        }
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! dbus_default_interface {
    ($interface:expr) => {
        #[allow(dead_code)]
        async fn get_property<R>(&self, name: &str) -> crate::Result<R>
        where
            R: for<'b> dbus::arg::Get<'b> + std::fmt::Debug + 'static,
        {
            self.get_property_with_interface(name, $interface).await
        }

        #[allow(dead_code)]
        async fn get_opt_property<R>(&self, name: &str) -> crate::Result<Option<R>>
        where
            R: for<'b> dbus::arg::Get<'b> + std::fmt::Debug + 'static,
        {
            self.get_opt_property_with_interface(name, $interface).await
        }

        #[allow(dead_code)]
        async fn set_property<T>(&self, name: &str, value: T) -> crate::Result<()>
        where
            T: dbus::arg::Arg + dbus::arg::Append + std::fmt::Debug,
        {
            self.set_property_with_interface(name, value, $interface).await
        }

        #[allow(dead_code)]
        async fn call_method<A, R>(&self, name: &str, args: A) -> crate::Result<R>
        where
            A: dbus::arg::AppendAll + std::fmt::Debug,
            R: dbus::arg::ReadAll + std::fmt::Debug + 'static,
        {
            self.call_method_with_interface(name, args, $interface).await
        }
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! define_properties {
    (@get
        $(#[$outer:meta])*
        $getter_name:ident, $dbus_name:expr, OPTIONAL ;
        $dbus_interface:expr, $dbus_value:ident : $dbus_type:ty => $getter_transform:block => $type:ty
    ) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> crate::Result<Option<$type>> {
            let dbus_opt_value: Option<$dbus_type> = self.get_opt_property_with_interface($dbus_name, $dbus_interface).await?;
            #[allow(clippy::manual_map)]
            let value: Option<$type> = match dbus_opt_value.as_ref() {
                Some($dbus_value) => Some($getter_transform),
                None => None
            };
            Ok(value)
        }
    };

    (@get
        $(#[$outer:meta])*
        $getter_name:ident, $dbus_name:expr, MANDATORY ;
        $dbus_interface:expr, $dbus_value:ident : $dbus_type:ty => $getter_transform:block => $type:ty
    ) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> crate::Result<$type> {
            let dbus_value: $dbus_type = self.get_property_with_interface($dbus_name, $dbus_interface).await?;
            let $dbus_value = &dbus_value;
            let value: $type = $getter_transform;
            Ok(value)
        }
    };

    (@set
        $(#[$outer:meta])*
        set: ($setter_name:ident, $value:ident => $setter_transform:block),,
        $dbus_interface:expr, $dbus_name:expr, $dbus_type:ty => $type:ty
    ) => {
        $(#[$outer])*
        pub async fn $setter_name(&self, $value: $type) -> crate::Result<()> {
            let dbus_value: $dbus_type = $setter_transform;
            self.set_property_with_interface($dbus_name, dbus_value, $dbus_interface).await?;
            Ok(())
        }
    };

    (@set
        $(#[$outer:meta])*
        ,
        $dbus_interface:expr, $dbus_name:expr, $dbus_type:ty => $type:ty
    ) => {};

    (@fetch_prop
        $self:ident, $props_name:ident, $enum_name:ident, $name:ident, $getter_name:ident, OPTIONAL
    ) => {
        if let Some(value) = $self.$getter_name().await? {
            $props_name.push($enum_name::$name(value));
        }
    };

    (@fetch_prop
        $self:ident, $props_name:ident, $enum_name:ident, $name:ident, $getter_name:ident, MANDATORY
    ) => {
        $props_name.push($enum_name::$name($self.$getter_name().await?));
    };

    (
        $struct_name:ident, $(#[$enum_outer:meta])* $enum_vis:vis $enum_name:ident =>
        {$(
            $(#[$outer:meta])*
            property(
                $name:ident, $type:ty,
                dbus: ($dbus_interface:expr, $dbus_name:expr, $dbus_type:ty, $opt:tt),
                get: ($getter_name:ident, $dbus_value:ident => $getter_transform:block),
                $( $set_tt:tt )*
            );
        )*}
    ) => {
        impl $struct_name {
            $(
                define_properties!(@get
                    $(#[$outer])*
                    $getter_name, $dbus_name, $opt ;
                    $dbus_interface, $dbus_value : $dbus_type => $getter_transform => $type
                );

                define_properties!(@set
                    $(#[$outer])*
                    $($set_tt)*,
                    $dbus_interface, $dbus_name, $dbus_type => $type
                );
            )*

            /// Queries and returns all properties.
            #[allow(dead_code)]
            $enum_vis async fn all_properties(&self) -> Result<Vec<$enum_name>> {
                let mut props = Vec::new();

                $(
                    define_properties!(@fetch_prop
                        self, props, $enum_name, $name, $getter_name, $opt
                    );
                )*

                Ok(props)
            }
        }

        $(#[$enum_outer])*
        #[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
        #[derive(Debug, Clone)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[non_exhaustive]
        $enum_vis enum $enum_name {
            $(
                $(#[$outer])*
                $name ($type),
            )*
        }

        impl $enum_name {
            #[allow(dead_code)]
            fn from_variant_property(
                name: &str,
                var_value: dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>
            ) -> crate::Result<Option<Self>> {
                match name {
                    $(
                        $dbus_name => {
                            crate::with_variant_property_cast(&var_value.0, |dbus_opt_value: Option<&$dbus_type>| {
                                match dbus_opt_value {
                                    Some($dbus_value) => {
                                        let value: $type = $getter_transform;
                                        Ok(Some(Self::$name (value)))
                                    },
                                    None => {
                                        log::warn!("Casting variant property {} with value {:?} failed",
                                            &name, &var_value.0);
                                        Ok(None)
                                    }
                                }
                            })
                        }
                    )*,
                    _ => Ok(None),
                }
            }

            #[allow(dead_code)]
            fn from_prop_map(prop_map: dbus::arg::PropMap) -> Vec<Self> {
                prop_map.into_iter().filter_map(|(name, value)|
                    Self::from_variant_property(&name, value).ok().flatten()
                ).collect()
            }
        }
    }
}

#[cfg(feature = "bluetoothd")]
fn variant_hashmap<K>(a: &(dyn RefArg + 'static)) -> HashMap<K, Variant<Box<dyn RefArg + 'static>>>
where
    K: std::hash::Hash + std::cmp::Eq + Clone + 'static,
{
    let mut hm: HashMap<K, Variant<Box<dyn RefArg + 'static>>> = HashMap::new();

    let mut key: Option<K> = None;
    for i in a.as_iter().unwrap() {
        let ib = i.box_clone();
        match key.take() {
            Some(key) => {
                let value: &Variant<Box<dyn RefArg + 'static>> = dbus::arg::cast(&ib).unwrap();
                hm.insert(key, Variant(value.0.box_clone()));
            }
            None => {
                let key_ref: Option<&K> = dbus::arg::cast(&ib);
                key = Some(key_ref.unwrap().clone());
            }
        }
    }

    hm
}

#[cfg(feature = "bluetoothd")]
pub(crate) fn with_variant_property_cast<T, R>(a: &(dyn RefArg + 'static), f: impl FnOnce(Option<&T>) -> R) -> R
where
    T: 'static,
{
    let dbus_cast: Option<&T> = dbus::arg::cast(a);
    match dbus_cast {
        Some(v) => f(Some(v)),
        None => {
            use std::any::Any;

            if a.signature().starts_with("a{yv") {
                let hm = variant_hashmap::<u8>(a);
                f((&hm as &dyn Any).downcast_ref())
            } else if a.signature().starts_with("a{qv") {
                let hm = variant_hashmap::<u16>(a);
                f((&hm as &dyn Any).downcast_ref())
            } else if a.signature().starts_with("a{sv") {
                let hm = variant_hashmap::<String>(a);
                f((&hm as &dyn Any).downcast_ref())
            } else {
                log::warn!("unimplemented D-Bus type signature: {}", a.signature());
                f(None)
            }
        }
    }
}

#[cfg(feature = "bluetoothd")]
macro_rules! cr_property {
    ($ib:expr, $dbus_name:expr, $obj:ident => $get:block) => {
        $ib.property($dbus_name).get(|ctx, $obj| {
            let value = $get;
            log::trace!("{}: {}.{} = {:?}", ctx.path(), ctx.interface(), &$dbus_name, &value);
            match value {
                Some(v) => Ok(v),
                None => Err(dbus_crossroads::MethodErr::no_property($dbus_name)),
            }
        })
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! define_flags {
    ($vis:vis $name:ident, $doc:tt => {
        $(
            $(#[$field_outer:meta])*
            $field:ident ($dbus_name:expr),
        )*
    }) => {
        #[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[doc=$doc]
        $vis struct $name {
            $(
                $(#[$field_outer])*
                pub $field: bool,
            )*
        }

        impl $name {
            #[allow(dead_code)]
            pub(crate) fn as_vec(&self) -> Vec<String> {
                let mut v = Vec::new();
                $(
                    if self.$field {
                        v.push($dbus_name.to_string());
                    }
                )*
                v
            }

            #[allow(dead_code)]
            pub(crate) fn from_slice(v: &[String]) -> Self {
                let hs: std::collections::HashSet<&str> = v.into_iter().map(|s| s.as_str()).collect();
                let mut s = Self::default();
                $(
                    if hs.contains($dbus_name) {
                        s.$field = true;
                    }
                )*
                s
            }
        }
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! read_prop {
    ($dict:expr, $name:expr, $type:ty) => {
        dbus::arg::prop_cast::<$type>($dict, $name).ok_or(MethodErr::invalid_arg($name))?.to_owned()
    };
}

#[cfg(feature = "bluetoothd")]
macro_rules! read_opt_prop {
    ($dict:expr, $name:expr, $type:ty) => {
        dbus::arg::prop_cast::<$type>($dict, $name).cloned()
    };
}

#[cfg(any(feature = "l2cap", feature = "rfcomm"))]
#[macro_use]
mod sock;

#[cfg(feature = "bluetoothd")]
mod adapter;
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
pub mod adv;
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
pub mod agent;
#[cfg(feature = "bluetoothd")]
mod device;
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
pub mod gatt;
#[cfg(feature = "l2cap")]
#[cfg_attr(docsrs, doc(cfg(feature = "l2cap")))]
pub mod l2cap;
#[cfg(feature = "rfcomm")]
#[cfg_attr(docsrs, doc(cfg(feature = "rfcomm")))]
pub mod rfcomm;
#[cfg(feature = "bluetoothd")]
mod session;
mod sys;
#[cfg(feature = "mesh")]
pub mod mesh;

#[cfg(feature = "bluetoothd")]
pub use crate::{adapter::*, device::*, session::*};

#[doc(no_inline)]
pub use uuid::Uuid;
mod uuid_ext;
pub use uuid_ext::UuidExt;

#[cfg(feature = "id")]
#[cfg_attr(docsrs, doc(cfg(feature = "id")))]
pub mod id;

/// Bluetooth error.
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Error {
    /// Error kind.
    pub kind: ErrorKind,
    /// Detailed error message provided by BlueZ.
    pub message: String,
}

/// Bluetooth error kind.
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ErrorKind {
    /// Bluetooth device already connected
    AlreadyConnected,
    /// Bluetooth device already exists
    AlreadyExists,
    /// Bluetooth authentication canceled
    AuthenticationCanceled,
    /// Bluetooth authentication failed
    AuthenticationFailed,
    /// Bluetooth authentication rejected
    AuthenticationRejected,
    /// Bluetooth authentication timeout
    AuthenticationTimeout,
    /// Bluetooth connection attempt failed
    ConnectionAttemptFailed,
    /// Bluetooth device does not exist
    DoesNotExist,
    /// Bluetooth operation failed
    Failed,
    /// Bluetooth operation in progress
    InProgress,
    /// Invalid arguments for Bluetooth operation
    InvalidArguments,
    /// the data provided is of invalid length
    InvalidLength,
    /// Bluetooth operation not available
    NotAvailable,
    /// Bluetooth operation not authorized
    NotAuthorized,
    /// Bluetooth device not ready
    NotReady,
    /// Bluetooth operation not supported
    NotSupported,
    /// Bluetooth operation not permitted
    NotPermitted,
    /// invalid offset for Bluetooth GATT property
    InvalidOffset,
    /// invalid Bluetooth address: {0}
    #[strum(disabled)]
    InvalidAddress(String),
    /// invalid Bluetooth adapter name: {0}
    #[strum(disabled)]
    InvalidName(String),
    /// GATT services have not been resolved for that Bluetooth device
    #[strum(disabled)]
    ServicesUnresolved,
    /// Bluetooth application is not registered
    #[strum(disabled)]
    NotRegistered,
    /// the receiving Bluetooth device has stopped the notification session
    #[strum(disabled)]
    NotificationSessionStopped,
    /// the indication was not confirmed by the receiving device
    #[strum(disabled)]
    IndicationUnconfirmed,
    /// the target object was either not present or removed
    #[strum(disabled)]
    NotFound,
    /// internal error: {0}
    #[strum(disabled)]
    Internal(InternalErrorKind),
}

/// Internal Bluetooth error kind.
///
/// This is most likely caused by incompatibilities between this library
/// and the version of the Bluetooth daemon.
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum InternalErrorKind {
    /// invalid UUID: {0}
    InvalidUuid(String),
    /// invalid value
    InvalidValue,
    /// invalid modalias: {0}
    InvalidModalias(String),
    /// key {0} is missing
    MissingKey(String),
    /// join error
    JoinError,
    /// IO error {0:?}
    // The error kind is not preserved during serialization.
    #[cfg_attr(feature = "serde", serde(with = "io_errorkind_serde"))]
    Io(std::io::ErrorKind),
    /// D-Bus error {0}
    DBus(String),
    /// lost connection to D-Bus
    DBusConnectionLost,
}

#[cfg(feature = "bluetoothd")]
impl Error {
    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self { kind, message: String::new() }
    }
}

#[cfg(feature = "bluetoothd")]
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", &self.kind)
        } else {
            write!(f, "{}: {}", &self.kind, &self.message)
        }
    }
}

#[cfg(feature = "bluetoothd")]
impl std::error::Error for Error {}

#[cfg(feature = "bluetoothd")]
impl From<dbus::Error> for Error {
    fn from(err: dbus::Error) -> Self {
        log::trace!("DBus error {}: {}", err.name().unwrap_or_default(), err.message().unwrap_or_default());
        if err.name() == Some("org.freedesktop.DBus.Error.UnknownObject") {
            return Self::new(ErrorKind::NotFound);
        }
        let kind = match err
            .name()
            .and_then(|name| name.strip_prefix(ERR_PREFIX))
            .and_then(|s| ErrorKind::from_str(s).ok())
        {
            Some(kind) => kind,
            _ => ErrorKind::Internal(InternalErrorKind::DBus(err.name().unwrap_or_default().to_string())),
        };
        Self { kind, message: err.message().unwrap_or_default().to_string() }
    }
}

#[cfg(feature = "bluetoothd")]
impl From<JoinError> for Error {
    fn from(err: JoinError) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::JoinError), message: err.to_string() }
    }
}

#[cfg(feature = "bluetoothd")]
impl From<strum::ParseError> for Error {
    fn from(_: strum::ParseError) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::InvalidValue), message: String::new() }
    }
}

#[cfg(feature = "bluetoothd")]
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::Io(err.kind())), message: err.to_string() }
    }
}

#[cfg(feature = "bluetoothd")]
impl From<InvalidAddress> for Error {
    fn from(err: InvalidAddress) -> Self {
        Self::new(ErrorKind::InvalidAddress(err.0))
    }
}

#[cfg(feature = "bluetoothd")]
impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        use std::io::ErrorKind as E;
        let kind = match err.kind {
            ErrorKind::AlreadyConnected => E::AlreadyExists,
            ErrorKind::AlreadyExists => E::AlreadyExists,
            ErrorKind::AuthenticationCanceled => E::PermissionDenied,
            ErrorKind::AuthenticationFailed => E::PermissionDenied,
            ErrorKind::AuthenticationRejected => E::PermissionDenied,
            ErrorKind::AuthenticationTimeout => E::PermissionDenied,
            ErrorKind::ConnectionAttemptFailed => E::ConnectionRefused,
            ErrorKind::DoesNotExist => E::NotFound,
            ErrorKind::Failed => E::Other,
            ErrorKind::InProgress => E::Other,
            ErrorKind::InvalidArguments => E::InvalidInput,
            ErrorKind::InvalidLength => E::InvalidData,
            ErrorKind::NotAvailable => E::NotFound,
            ErrorKind::NotAuthorized => E::PermissionDenied,
            ErrorKind::NotReady => E::Other,
            ErrorKind::NotSupported => E::Unsupported,
            ErrorKind::NotPermitted => E::PermissionDenied,
            ErrorKind::InvalidOffset => E::InvalidInput,
            ErrorKind::InvalidAddress(_) => E::InvalidInput,
            ErrorKind::InvalidName(_) => E::InvalidInput,
            ErrorKind::ServicesUnresolved => E::Other,
            ErrorKind::NotRegistered => E::Other,
            ErrorKind::NotificationSessionStopped => E::ConnectionReset,
            ErrorKind::IndicationUnconfirmed => E::TimedOut,
            ErrorKind::NotFound => E::NotFound,
            ErrorKind::Internal(InternalErrorKind::Io(err)) => err,
            ErrorKind::Internal(_) => E::Other,
        };
        std::io::Error::new(kind, err)
    }
}

#[cfg(all(feature = "bluetoothd", feature = "serde"))]
mod io_errorkind_serde {
    pub fn serialize<S>(_kind: &std::io::ErrorKind, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::Serialize;
        ().serialize(ser)
    }

    pub fn deserialize<'de, D>(deser: D) -> Result<std::io::ErrorKind, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        <()>::deserialize(deser)?;
        Ok(std::io::ErrorKind::Other)
    }
}

/// Bluetooth result.
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
pub type Result<T> = std::result::Result<T, Error>;

/// Bluetooth address.
///
/// The serialized representation is a string in colon-hexadecimal notation.
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Address(pub [u8; 6]);

impl Address {
    /// Creates a new Bluetooth address with the specified value.
    pub const fn new(addr: [u8; 6]) -> Self {
        Self(addr)
    }

    /// Any Bluetooth address.
    ///
    /// Corresponds to `00:00:00:00:00:00`.
    pub const fn any() -> Self {
        Self([0; 6])
    }
}

impl Deref for Address {
    type Target = [u8; 6];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Address {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl Debug for Address {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl From<sys::bdaddr_t> for Address {
    fn from(mut addr: sys::bdaddr_t) -> Self {
        addr.b.reverse();
        Self(addr.b)
    }
}

impl From<Address> for sys::bdaddr_t {
    fn from(mut addr: Address) -> Self {
        addr.0.reverse();
        sys::bdaddr_t { b: addr.0 }
    }
}

impl From<MacAddr6> for Address {
    fn from(addr: MacAddr6) -> Self {
        Self(addr.into_array())
    }
}

impl From<Address> for MacAddr6 {
    fn from(addr: Address) -> Self {
        addr.0.into()
    }
}

/// Invalid Bluetooth address error.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvalidAddress(pub String);

impl fmt::Display for InvalidAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "invalid Bluetooth address: {}", &self.0)
    }
}

impl std::error::Error for InvalidAddress {}

impl FromStr for Address {
    type Err = InvalidAddress;
    fn from_str(s: &str) -> std::result::Result<Self, InvalidAddress> {
        let fields = s
            .split(':')
            .map(|s| u8::from_str_radix(s, 16).map_err(|_| InvalidAddress(s.to_string())))
            .collect::<std::result::Result<Vec<_>, InvalidAddress>>()?;
        Ok(Self(fields.try_into().map_err(|_| InvalidAddress(s.to_string()))?))
    }
}

impl From<[u8; 6]> for Address {
    fn from(addr: [u8; 6]) -> Self {
        Self(addr)
    }
}

impl From<Address> for [u8; 6] {
    fn from(addr: Address) -> Self {
        addr.0
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(D::Error::custom)
    }
}

/// Bluetooth device address type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Display, EnumString, FromPrimitive)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum AddressType {
    /// Classic Bluetooth (BR/EDR) address.
    #[strum(serialize = "br/edr")]
    BrEdr = sys::BDADDR_BREDR,
    /// Bluetooth Low Energy (LE) public address.
    #[strum(serialize = "public")]
    LePublic = sys::BDADDR_LE_PUBLIC,
    /// Bluetooth Low Energy (LE) random address.
    #[strum(serialize = "random")]
    LeRandom = sys::BDADDR_LE_RANDOM,
}

impl Default for AddressType {
    fn default() -> Self {
        Self::LePublic
    }
}

/// Linux kernel modalias information.
#[cfg(feature = "bluetoothd")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetoothd")))]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Modalias {
    /// Source.
    pub source: String,
    /// Vendor id.
    pub vendor: u32,
    /// Product id.
    pub product: u32,
    /// Device id.
    pub device: u32,
}

#[cfg(feature = "bluetoothd")]
impl FromStr for Modalias {
    type Err = Error;

    fn from_str(m: &str) -> Result<Self> {
        fn do_parse(m: &str) -> Option<Modalias> {
            let ids: Vec<&str> = m.split(':').collect();

            #[allow(clippy::get_first)]
            let source = ids.get(0)?;
            let vendor = Vec::from_hex(ids.get(1)?.get(1..5)?).ok()?;
            let product = Vec::from_hex(ids.get(1)?.get(6..10)?).ok()?;
            let device = Vec::from_hex(ids.get(1)?.get(11..15)?).ok()?;

            Some(Modalias {
                source: source.to_string(),
                vendor: (vendor[0] as u32) << 8 | (vendor[1] as u32),
                product: (product[0] as u32) << 8 | (product[1] as u32),
                device: (device[0] as u32) << 8 | (device[1] as u32),
            })
        }
        do_parse(m)
            .ok_or_else(|| Error::new(ErrorKind::Internal(InternalErrorKind::InvalidModalias(m.to_string()))))
    }
}

/// Gets all D-Bus objects from the BlueZ service.
#[cfg(feature = "bluetoothd")]
async fn all_dbus_objects(
    connection: &SyncConnection,
) -> Result<HashMap<Path<'static>, HashMap<String, PropMap>>> {
    let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, connection);
    Ok(p.get_managed_objects().await?)
}

/// Read value from D-Bus dictionary.
#[cfg(feature = "bluetoothd")]
pub(crate) fn read_dict<'a, T: 'static>(
    dict: &'a HashMap<String, Variant<Box<dyn RefArg + 'static>>>, key: &str,
) -> Result<&'a T> {
    prop_cast(dict, key)
        .ok_or_else(|| Error::new(ErrorKind::Internal(InternalErrorKind::MissingKey(key.to_string()))))
}

/// Returns the parent path of the specified D-Bus path.
#[cfg(feature = "bluetoothd")]
pub(crate) fn parent_path<'a>(path: &Path<'a>) -> Path<'a> {
    let mut comps: Vec<_> = path.split('/').collect();
    comps.pop();
    if comps.is_empty() {
        Path::new("/").unwrap()
    } else {
        Path::new(comps.join("/")).unwrap()
    }
}

/// Result of calling one of our D-Bus methods.
#[cfg(feature = "bluetoothd")]
type DbusResult<T> = std::result::Result<T, dbus::MethodErr>;

/// Call method on Arc D-Bus object we are serving.
#[cfg(feature = "bluetoothd")]
fn method_call<
    T: Send + Sync + 'static,
    R: AppendAll + fmt::Debug,
    F: Future<Output = DbusResult<R>> + Send + 'static,
>(
    mut ctx: Context, cr: &mut Crossroads, f: impl FnOnce(Arc<T>) -> F,
) -> impl Future<Output = PhantomData<R>> {
    let data_ref: &mut Arc<T> = cr.data_mut(ctx.path()).unwrap();
    let data: Arc<T> = data_ref.clone();
    async move {
        if log::log_enabled!(log::Level::Trace) {
            let mut args = Vec::new();
            let mut arg_iter = ctx.message().iter_init();
            while let Some(value) = arg_iter.get_refarg() {
                args.push(format!("{value:?}"));
                arg_iter.next();
            }
            log::trace!(
                "{}: {}.{} ({})",
                ctx.path(),
                ctx.interface().map(|i| i.to_string()).unwrap_or_default(),
                ctx.method(),
                args.join(", ")
            );
        }
        let result = f(data).await;
        log::trace!(
            "{}: {}.{} (...) -> {:?}",
            ctx.path(),
            ctx.interface().map(|i| i.to_string()).unwrap_or_default(),
            ctx.method(),
            &result
        );
        ctx.reply(result)
    }
}
