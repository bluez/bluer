//! # BLEZ - Asynchronous Bluetooth Low Energy on Linux
//!
//! This library provides an asynchronous, fully featured interface to the [Bluetooth Low Energy (BLE)](https://en.wikipedia.org/wiki/Bluetooth_Low_Energy)
//! APIs of the [official Linux Bluetooth protocol stack (BlueZ)](http://www.bluez.org/).
//! Both publishing local and consuming remote GATT services using *idiotmatic* Rust code is supported.
//!
//! The following features are provided:
//!
//! * Bluetooth adapters
//!     * enumeration
//!     * configuration of power, discoverability, name, etc.
//!     * hotplug support through change events stream
//! * Bluetooth devices
//!     * discovery
//!     * querying of address, name, class, signal strength (RSSI), etc.
//!     * Bluetooth Low Energy advertisements
//!     * change events stream
//!     * connecting and pairing
//! * consumption of remote GATT services
//!     * GATT service discovery
//!     * read, write and notify operations on characteristics
//!     * read and write operations on characteristic descriptors
//!     * optional use of low-overhead `AsyncRead` and `AsyncWrite` streams for notify and write operations
//! * publishing local GATT services
//!     * read, write and notify operations on characteristics
//!     * read and write operations on characteristic descriptors
//!     * two programming models supported
//!         * callback-based interface
//!         * low-overhead `AsyncRead` and `AsyncWrite` streams
//! * sending Bluetooth Low Energy advertisements
//!
//! Classic Bluetooth is unsupported except for device discovery.
//!
//! ## Basic usage
//! Create a [Session] using [Session::new].
//! Then obtain a Bluetooth adapter using [Session::adapter].
//! From there on you can access most of the functionality using the methods provided by [Adapter].

use dbus::{
    arg::{prop_cast, OwnedFd, PropMap, RefArg, Variant},
    nonblock::{stdintf::org_freedesktop_dbus::ObjectManager, Proxy, SyncConnection},
    Path,
};
use hex::FromHex;
use libc::{c_int, socketpair, AF_LOCAL, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_SEQPACKET};
use std::{
    collections::HashMap,
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    os::unix::prelude::{FromRawFd, RawFd},
    str::FromStr,
    time::Duration,
};
use strum::{Display, EnumString};
use tokio::{net::UnixStream, task::JoinError};

pub(crate) const SERVICE_NAME: &str = "org.bluez";
pub(crate) const ERR_PREFIX: &str = "org.bluez.Error.";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

macro_rules! publish_path {
    ($path:expr) => {
        concat!("/io/crates/", env!("CARGO_PKG_NAME"), "/", $path)
    };
}

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

macro_rules! define_properties {
    (@get
        $(#[$outer:meta])*
        $getter_name:ident, $dbus_name:expr, OPTIONAL ;
        $dbus_interface:expr, $dbus_value:ident : $dbus_type:ty => $getter_transform:block => $type:ty
    ) => {
        $(#[$outer])*
        pub async fn $getter_name(&self) -> crate::Result<Option<$type>> {
            let dbus_opt_value: Option<$dbus_type> = self.get_opt_property_with_interface($dbus_name, $dbus_interface).await?;
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
        }

        $(#[$enum_outer])*
        #[derive(Debug, Clone)]
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
                            let dbus_opt_value: Option<&$dbus_type> = dbus::arg::cast(&var_value.0);
                            match dbus_opt_value {
                                Some($dbus_value) => {
                                    let value: $type = $getter_transform;
                                    Ok(Some(Self::$name (value)))
                                },
                                None => Ok(None),
                            }
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

macro_rules! define_flags {
    ($name:ident, $doc:tt => {
        $(
            $(#[$field_outer:meta])*
            $field:ident ($dbus_name:expr),
        )*
    }) => {
        #[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        #[doc=$doc]
        pub struct $name {
            $(
                $(#[$field_outer])*
                pub $field: bool,
            )*
        }

        impl $name {
            #[allow(dead_code)]
            pub(crate) fn to_vec(&self) -> Vec<String> {
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

macro_rules! read_prop {
    ($dict:expr, $name:expr, $type:ty) => {
        dbus::arg::prop_cast::<$type>($dict, $name).ok_or(MethodErr::invalid_arg($name))?.to_owned()
    };
}

macro_rules! read_opt_prop {
    ($dict:expr, $name:expr, $type:ty) => {
        dbus::arg::prop_cast::<$type>($dict, $name).cloned()
    };
}

mod adapter;
pub mod adv;
mod device;
pub mod gatt;
mod session;

pub use crate::{adapter::*, device::*, session::*};
pub use uuid::Uuid;

/// Bluetooth error.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Error {
    /// Error kind
    pub kind: ErrorKind,
    /// Message
    pub message: String,
}

/// Bluetooth error kind.
#[derive(Clone, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Bluetooth device already connected.
    AlreadyConnected,
    /// Bluetooth device already exists.
    AlreadyExists,
    /// Bluetooth authentication canceled.
    AuthenticationCanceled,
    /// Bluetooth authentication failed.
    AuthenticationFailed,
    /// Bluetooth authentication rejected.
    AuthenticationRejected,
    /// Bluetooth authentication timeout.
    AuthenticationTimeout,
    /// Bluetooth connection attempt failed.
    ConnectionAttemptFailed,
    /// Bluetooth device does not exist.
    DoesNotExist,
    /// Bluetooth operation failed.
    Failed,
    /// Bluetooth operation in progress.
    InProgress,
    /// Invalid arguments for Bluetooth operation.
    InvalidArguments,
    /// The data provided is of invalid length.
    InvalidLength,
    /// Bluetooth operation not available.
    NotAvailable,
    /// Bluetooth operation not authorized.
    NotAuthorized,
    /// Bluetooth device not ready.
    NotReady,
    /// Bluetooth operation not supported.
    NotSupported,
    /// Bluetooth operation not permitted.
    NotPermitted,
    /// Invalid offset for Bluetooth GATT property.
    InvalidOffset,
    /// D-Bus error {0}.
    #[strum(disabled)]
    DBus(String),
    /// Lost connection to D-Bus.
    #[strum(disabled)]
    DBusConnectionLost,
    /// Invalid Bluetooth address: {0}.
    #[strum(disabled)]
    InvalidAddress(String),
    /// Invalid Bluetooth adapter name: {0}.
    #[strum(disabled)]
    InvalidName(String),
    /// GATT services have not been resolved for that Bluetooth device.
    #[strum(disabled)]
    ServicesUnresolved,
    /// Bluetooth application is not registered.
    #[strum(disabled)]
    NotRegistered,
    /// The receiving Bluetooth device has stopped the notification session.
    #[strum(disabled)]
    NotificationSessionStopped,
    /// The indication was not confirmed by the receiving device.
    #[strum(disabled)]
    IndicationUnconfirmed,
    /// The target object was either not present or removed.
    #[strum(disabled)]
    NotFound,
    /// Internal error: {0}
    #[strum(disabled)]
    Internal(InternalErrorKind),
}

/// Internal Bluetooth error kind.
///
/// This is most likely caused by incompatibilies between this library
/// and the version of the Bluetooth daemon.
#[derive(Clone, Debug, displaydoc::Display, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[non_exhaustive]
pub enum InternalErrorKind {
    /// Invalid UUID: {0}.
    InvalidUuid(String),
    /// Invalid value.
    InvalidValue,
    /// Invalid modalias: {0}.
    InvalidModalias(String),
    /// Key {0} is missing.
    MissingKey(String),
    /// Join error.
    JoinError,
    /// IO error {0:?}.
    Io(std::io::ErrorKind),
}

impl Error {
    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self { kind, message: String::new() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", &self.kind)
        } else {
            write!(f, "{}: {}", &self.kind, &self.message)
        }
    }
}

impl std::error::Error for Error {}

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
            _ => ErrorKind::DBus(err.name().unwrap_or_default().to_string()),
        };
        Self { kind, message: err.message().unwrap_or_default().to_string() }
    }
}

impl From<JoinError> for Error {
    fn from(err: JoinError) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::JoinError), message: err.to_string() }
    }
}

impl From<strum::ParseError> for Error {
    fn from(_: strum::ParseError) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::InvalidValue), message: String::new() }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self { kind: ErrorKind::Internal(InternalErrorKind::Io(err.kind())), message: err.to_string() }
    }
}

/// Bluetooth result.
pub type Result<T> = std::result::Result<T, Error>;

/// Bluetooth address.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Address(pub [u8; 6]);

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
        write!(f, "{}", self)
    }
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let fields = s
            .split(':')
            .map(|s| u8::from_str_radix(s, 16).map_err(|_| Error::new(ErrorKind::InvalidAddress(s.to_string()))))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(fields.try_into().map_err(|_| Error::new(ErrorKind::InvalidAddress(s.to_string())))?))
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

/// Bluetooth device address type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Display, EnumString)]
pub enum AddressType {
    /// Public address
    #[strum(serialize = "public")]
    Public,
    /// Random address
    #[strum(serialize = "random")]
    Random,
}

/// Linux kernel modalias information.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
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

impl FromStr for Modalias {
    type Err = Error;

    fn from_str(m: &str) -> Result<Self> {
        fn do_parse(m: &str) -> Option<Modalias> {
            let ids: Vec<&str> = m.split(':').collect();

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

/// Gets all D-Bus objects from the bluez service.
async fn all_dbus_objects(
    connection: &SyncConnection,
) -> Result<HashMap<Path<'static>, HashMap<String, PropMap>>> {
    let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, connection);
    Ok(p.get_managed_objects().await?)
}

/// Read value from D-Bus dictionary.
pub(crate) fn read_dict<'a, T: 'static>(
    dict: &'a HashMap<String, Variant<Box<dyn RefArg + 'static>>>, key: &str,
) -> Result<&'a T> {
    prop_cast(dict, key).ok_or(Error::new(ErrorKind::Internal(InternalErrorKind::MissingKey(key.to_string()))))
}

/// Creates a UNIX socket pair.
pub(crate) fn make_socket_pair() -> std::result::Result<(OwnedFd, UnixStream), std::io::Error> {
    let mut sv: [RawFd; 2] = [0; 2];
    unsafe {
        if socketpair(AF_LOCAL, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, &mut sv as *mut c_int) == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    let [fd1, fd2] = sv;

    let fd1 = unsafe { OwnedFd::new(fd1) };
    let us = UnixStream::from_std(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd2) })?;

    Ok((fd1, us))
}

/// Returns the parent path of the specified D-Bus path.
pub(crate) fn parent_path<'a>(path: &Path<'a>) -> Path<'a> {
    let mut comps: Vec<_> = path.split('/').collect();
    comps.pop();
    if comps.is_empty() {
        Path::new("/").unwrap()
    } else {
        Path::new(comps.join("/")).unwrap()
    }
}

/// UUID extension trait to convert to and from Bluetooth short UUIDs.
pub trait UuidExt {
    /// 32-bit short form of Bluetooth UUID.
    fn as_u32(&self) -> Option<u32>;
    /// 16-bit short form of Bluetooth UUID.
    fn as_u16(&self) -> Option<u16>;
    /// Long form of 32-bit short form Bluetooth UUID.
    fn from_u32(v: u32) -> Uuid;
    /// Long form of 16-bit short form Bluetooth UUID.
    fn from_u16(v: u16) -> Uuid;
}

const BASE_UUID: u128 = 0x00000000_0000_1000_8000_00805f9b34fb;
const BASE_MASK_32: u128 = 0x00000000_ffff_ffff_ffff_ffffffffffff;
const BASE_MASK_16: u128 = 0xffff0000_ffff_ffff_ffff_ffffffffffff;

impl UuidExt for Uuid {
    fn as_u32(&self) -> Option<u32> {
        let value = self.as_u128();
        if value & BASE_MASK_32 == BASE_UUID {
            Some((value >> 96) as u32)
        } else {
            None
        }
    }

    fn as_u16(&self) -> Option<u16> {
        let value = self.as_u128();
        if value & BASE_MASK_16 == BASE_UUID {
            Some((value >> 96) as u16)
        } else {
            None
        }
    }

    fn from_u32(v: u32) -> Uuid {
        Uuid::from_u128(BASE_UUID | ((v as u128) << 96))
    }

    fn from_u16(v: u16) -> Uuid {
        Uuid::from_u128(BASE_UUID | ((v as u128) << 96))
    }
}
