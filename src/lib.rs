use dbus::{
    arg::{prop_cast, PropMap, RefArg, Variant},
    nonblock::{
        stdintf::org_freedesktop_dbus::{
            ObjectManager, ObjectManagerInterfacesAdded, ObjectManagerInterfacesRemoved,
            PropertiesPropertiesChanged,
        },
        Proxy, SyncConnection,
    },
    strings::BusName,
    Path,
};
use futures::{channel::mpsc, stream, SinkExt, StreamExt};
use hex::FromHex;
use lazy_static::lazy_static;
use std::{
    collections::HashMap,
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use strum::{Display, EnumString};
use thiserror::Error;
use tokio::task::JoinError;

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
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            Ok(self.proxy().get(interface, name).await?)
        }

        #[allow(dead_code)]
        async fn get_opt_property_with_interface<R>(
            &self, name: &str, interface: &str,
        ) -> crate::Result<Option<R>>
        where
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            match self.proxy().get(interface, name).await {
                Ok(v) => Ok(Some(v)),
                Err(err) if err.name() == Some("org.freedesktop.DBus.Error.InvalidArgs") => Ok(None),
                Err(err) => Err(err.into()),
            }
        }

        #[allow(dead_code)]
        async fn set_property_with_interface<T>(&self, name: &str, value: T, interface: &str) -> crate::Result<()>
        where
            T: dbus::arg::Arg + dbus::arg::Append,
        {
            use dbus::nonblock::stdintf::org_freedesktop_dbus::Properties;
            self.proxy().set(interface, name, value).await?;
            Ok(())
        }

        #[allow(dead_code)]
        async fn call_method_with_interface<A, R>(&self, name: &str, args: A, interface: &str) -> crate::Result<R>
        where
            A: dbus::arg::AppendAll,
            R: dbus::arg::ReadAll + 'static,
        {
            Ok(self.proxy().method_call(interface, name, args).await?)
        }
    };
}

macro_rules! dbus_default_interface {
    ($interface:expr) => {
        #[allow(dead_code)]
        async fn get_property<R>(&self, name: &str) -> crate::Result<R>
        where
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            self.get_property_with_interface(name, $interface).await
        }

        #[allow(dead_code)]
        async fn get_opt_property<R>(&self, name: &str) -> crate::Result<Option<R>>
        where
            R: for<'b> dbus::arg::Get<'b> + 'static,
        {
            self.get_opt_property_with_interface(name, $interface).await
        }

        #[allow(dead_code)]
        async fn set_property<T>(&self, name: &str, value: T) -> crate::Result<()>
        where
            T: dbus::arg::Arg + dbus::arg::Append,
        {
            self.set_property_with_interface(name, value, $interface).await
        }

        #[allow(dead_code)]
        async fn call_method<A, R>(&self, name: &str, args: A) -> crate::Result<R>
        where
            A: dbus::arg::AppendAll,
            R: dbus::arg::ReadAll + 'static,
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
        $struct_name:ident, $enum_name:ident =>
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
        pub enum $enum_name {
            $(
                $(#[$outer])*
                $name ($type),
            )*
        }

        impl $enum_name {
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
            pub(crate) fn from_vec(v: Vec<String>) -> Self {
                let hs: std::collections::HashSet<_> = v.into_iter().collect();
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

mod adapter;
mod advertising;
mod device;
pub mod gatt;
//mod bluetooth_discovery_session;
//mod bluetooth_event;
//mod bluetooth_gatt_characteristic;
//mod bluetooth_gatt_descriptor;
//mod bluetooth_gatt_service;
//mod bluetooth_le_advertising_data;
//mod bluetooth_le_advertising_manager;
//mod bluetooth_obex;
//mod bluetooth_utils;
mod session;

pub use crate::{adapter::*, advertising::*, device::*, session::*};
// pub use crate::bluetooth_gatt_characteristic::BluetoothGATTCharacteristic;
// pub use crate::bluetooth_gatt_descriptor::BluetoothGATTDescriptor;
// pub use crate::bluetooth_gatt_service::BluetoothGATTService;
// pub use crate::bluetooth_le_advertising_data::BluetoothAdvertisingData;
// pub use crate::bluetooth_le_advertising_manager::BluetoothAdvertisingManager;
// pub use crate::bluetooth_obex::BluetoothOBEXSession;

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
    #[error("Another Bluetooth device discovery is in progress")]
    AnotherDiscoveryInProgress,
    #[error("Bluetooth error: {0}")]
    Other(String),
}

impl From<dbus::Error> for Error {
    fn from(err: dbus::Error) -> Self {
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

/// Bluetooth result.
pub type Result<T> = std::result::Result<T, Error>;

/// Bluetooth address.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Address([u8; 6]);

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

/// D-Bus object event.
#[derive(Debug, Clone)]
pub(crate) enum ObjectEvent {
    /// Object or object interfaces added.
    Added { object: Path<'static>, interfaces: Vec<String> },
    /// Object or object interfaces removed.
    Removed { object: Path<'static>, interfaces: Vec<String> },
}

impl ObjectEvent {
    /// Stream D-Bus object events starting with specified path prefix.
    pub(crate) async fn stream(
        connection: Arc<SyncConnection>, path_prefix: Option<Path<'static>>,
    ) -> Result<mpsc::UnboundedReceiver<Self>> {
        use dbus::message::SignalArgs;
        lazy_static! {
            static ref SERVICE_NAME_BUS: BusName<'static> = BusName::new(SERVICE_NAME).unwrap();
            static ref SERVICE_NAME_REF: Option<&'static BusName<'static>> = Some(&SERVICE_NAME_BUS);
        }

        //let rule_add = ObjectManagerInterfacesAdded::match_rule(*SERVICE_NAME_REF, path_prefix.as_ref()).static_clone();
        let rule_add = ObjectManagerInterfacesAdded::match_rule(*SERVICE_NAME_REF, None).static_clone();
        let msg_match_add = connection.add_match(rule_add).await?;
        let (msg_match_add, stream_add) = msg_match_add.msg_stream();

        //let rule_removed = ObjectManagerInterfacesRemoved::match_rule(*SERVICE_NAME_REF, path_prefix.as_ref()).static_clone();
        let rule_removed = ObjectManagerInterfacesRemoved::match_rule(*SERVICE_NAME_REF, None).static_clone();
        let msg_match_removed = connection.add_match(rule_removed).await?;
        let (msg_match_removed, stream_removed) = msg_match_removed.msg_stream();

        let mut stream = stream::select(stream_add, stream_removed);

        let has_prefix = move |path: &Path<'static>| match &path_prefix {
            Some(prefix) => path.starts_with(&prefix.to_string()),
            None => true,
        };

        let (mut tx, rx) = mpsc::unbounded();
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let to_send = {
                    if let Some(ObjectManagerInterfacesAdded { object, interfaces, .. }) =
                        ObjectManagerInterfacesAdded::from_message(&msg)
                    {
                        if has_prefix(&object) {
                            Some(Self::Added {
                                object,
                                interfaces: interfaces.into_iter().map(|(interface, _)| interface).collect(),
                            })
                        } else {
                            None
                        }
                    } else if let Some(ObjectManagerInterfacesRemoved { object, interfaces, .. }) =
                        ObjectManagerInterfacesRemoved::from_message(&msg)
                    {
                        if has_prefix(&object) {
                            Some(Self::Removed { object, interfaces })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(msg) = to_send {
                    if tx.send(msg).await.is_err() {
                        break;
                    }
                }
            }

            let _ = connection.remove_match(msg_match_add.token()).await;
            let _ = connection.remove_match(msg_match_removed.token()).await;
        });

        Ok(rx)
    }
}

/// D-Bus property changed event.
#[derive(Debug)]
pub(crate) struct PropertyEvent {
    pub interface: String,
    pub changed: dbus::arg::PropMap,
}

impl PropertyEvent {
    /// Stream D-Bus property changed events.
    pub async fn stream(
        connection: Arc<SyncConnection>, path: Path<'static>,
    ) -> Result<mpsc::UnboundedReceiver<Self>> {
        use dbus::message::SignalArgs;
        lazy_static! {
            static ref SERVICE_NAME_BUS: BusName<'static> = BusName::new(SERVICE_NAME).unwrap();
            static ref SERVICE_NAME_REF: Option<&'static BusName<'static>> = Some(&SERVICE_NAME_BUS);
        }

        let rule = PropertiesPropertiesChanged::match_rule(*SERVICE_NAME_REF, Some(&path)).static_clone();
        let msg_match = connection.add_match(rule).await?;
        let (msg_match, mut stream) = msg_match.stream();

        let (mut tx, rx) = mpsc::unbounded();
        tokio::spawn(async move {
            while let Some((_, PropertiesPropertiesChanged { interface_name, changed_properties, .. })) =
                stream.next().await
            {
                let evt = Self { interface: interface_name, changed: changed_properties };

                if tx.send(evt).await.is_err() {
                    break;
                }
            }

            let _ = connection.remove_match(msg_match.token()).await;
        });

        Ok(rx)
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
    prop_cast(dict, key).ok_or(Error::MissingKey(key.to_string()))
}
