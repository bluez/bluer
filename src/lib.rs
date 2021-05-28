//! BLEZ - Asynchronous Bluetooth Low Energy using BlueZ
//! ====================================================

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
use thiserror::Error;
use tokio::{net::UnixStream, task::JoinError};

macro_rules! other_err {
    ($($e:tt)*) => {
        crate::Error::Other(format!($($e)*))
    };
}

pub(crate) const SERVICE_NAME: &str = "org.bluez";
pub(crate) const ERR_PREFIX: &str = "org.bluez.Error.";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(120);

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
        $struct_name:ident, $enum_vis:vis $enum_name:ident =>
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

        /// Property with value.
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
        $ib.property($dbus_name).get(|_, $obj| {
            eprintln!("Property {} queried", $dbus_name);
            match $get {
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
mod advertising;
mod device;
pub mod gatt;
mod session;

pub use crate::{adapter::*, advertising::*, device::*, session::*};

/// Bluetooth error.
#[derive(Clone, Debug, Error, EnumString)]
pub enum Error {
    #[error("Bluetooth device already connected")]
    AlreadyConnected,
    #[error("Bluetooth device already exists")]
    AlreadyExists,
    #[error("Bluetooth authentication canceled")]
    AuthenticationCanceled,
    #[error("Bluetooth authentication failed")]
    AuthenticationFailed,
    #[error("Bluetooth authentication rejected")]
    AuthenticationRejected,
    #[error("Bluetooth authentication timeout")]
    AuthenticationTimeout,
    #[error("Bluetooth connection attempt failed")]
    ConnectionAttemptFailed,
    #[error("Bluetooth device does not exist")]
    DoesNotExist,
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("invalid arguments for Bluetooth operation")]
    InvalidArguments,
    #[error("the data provided generates a Bluetooth data packet which is too long")]
    InvalidLength,
    #[error("Bluetooth operation not available")]
    NotAvailable,
    #[error("Bluetooth operation not authorized")]
    NotAuthorized,
    #[error("Bluetooth device not ready")]
    NotReady,
    #[error("Bluetooth operation not supported")]
    NotSupported,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Invalid offset for Bluetooth GATT property")]
    InvalidOffset,
    #[error("Bluetooth D-Bus error {name}: {message}")]
    DBus { name: String, message: String },
    #[error("Lost connection to D-Bus")]
    DBusConnectionLost,
    #[error("No Bluetooth adapter available")]
    NoAdapterAvailable,
    #[error("Bluetooth adapter {0} is not available")]
    AdapterNotAvailable(String),
    #[error("Join error: {0}")]
    JoinError(String),
    #[error("Invalid Bluetooth address: {0}")]
    InvalidAddress(String),
    #[error("Invalid Bluetooth adapter name: {0}")]
    InvalidName(String),
    #[error("Invalid UUID: {0}")]
    InvalidUuid(String),
    #[error("Invalid value")]
    InvalidValue,
    #[error("Key {0} is missing")]
    MissingKey(String),
    #[error("GATT services have not been resolved for that Bluetooth device")]
    ServicesUnresolved,
    #[error("Bluetooth application is not registered")]
    NotRegistered,
    #[error("The receiving Bluetooth device has stopped the notification session")]
    NotificationSessionStopped,
    #[error("The indication was not confirmed by the receiving device")]
    IndicationUnconfirmed,
    #[error("IO error {kind:?}: {msg}")]
    #[strum(disabled)]
    Io { kind: std::io::ErrorKind, msg: String },
    #[error("Bluetooth error: {0}")]
    Other(String),
}

impl From<dbus::Error> for Error {
    fn from(err: dbus::Error) -> Self {
        eprintln!("DBus error {}: {}", err.name().unwrap_or_default(), err.message().unwrap_or_default());
        match err.name().and_then(|name| name.strip_prefix(ERR_PREFIX)).and_then(|s| Self::from_str(s).ok()) {
            Some(err) => err,
            _ => Self::DBus {
                name: err.name().unwrap_or_default().to_string(),
                message: err.message().unwrap_or_default().to_string(),
            },
        }
    }
}

impl From<JoinError> for Error {
    fn from(err: JoinError) -> Self {
        Self::JoinError(err.to_string())
    }
}

impl From<strum::ParseError> for Error {
    fn from(_: strum::ParseError) -> Self {
        Self::InvalidValue
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io { kind: err.kind(), msg: err.to_string() }
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
            .map(|s| u8::from_str_radix(s, 16).map_err(|_| Error::InvalidAddress(s.to_string())))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(fields.try_into().map_err(|_| Error::InvalidAddress(s.to_string()))?))
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
        do_parse(m).ok_or_else(|| other_err!("invalid modalias: {}", m))
    }
}

/// Link type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Display, EnumString)]
pub enum LinkType {
    /// BR/EDR
    #[strum(serialize = "BR/EDR")]
    BrEdr,
    /// LE
    #[strum(serialize = "LE")]
    Le,
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
    prop_cast(dict, key).ok_or(Error::MissingKey(key.to_string()))
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
