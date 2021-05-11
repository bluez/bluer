use std::{fmt, marker::PhantomData, pin::Pin, sync::Arc};

use dbus::{
    arg::{prop_cast, AppendAll, PropMap, RefArg, Variant},
    nonblock::Proxy,
    MethodErr,
};
use dbus_crossroads::{Context, Crossroads, IfaceBuilder, IfaceToken};
use futures::{channel::oneshot, Future};
use strum::{Display, EnumString, IntoStaticStr};
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{Adapter, SessionInner, ERR_PREFIX, TIMEOUT};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.GattManager1";
pub(crate) const GATT_PREFIX: &str = "/io/crates/tokio_bluez/gatt/";

/// Local GATT service exposed over Bluetooth.
pub struct Service {
    /// 128-bit service UUID.
    pub uuid: Uuid,
    /// Indicates whether or not this GATT service is a
    /// primary service.
    ///
    /// If false, the service is secondary.
    pub primary: bool,
    /// List of GATT characteristics to expose.
    pub characteristics: Vec<Characteristic>,
}

impl Service {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattService1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", s => {
                Some(s.uuid.to_string())
            });
            cr_property!(ib, "Primary", s => {
                Some(s.primary)
            });
        })
    }
}

/// Read value request.
#[derive(Debug, Clone)]
pub struct ReadCharacteristicValueRequest {
    /// Offset.
    pub offset: u16,
    /// Exchanged MTU.
    pub mtu: u16,
}

impl ReadCharacteristicValueRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self { offset: read_prop!(dict, "offset", u16), mtu: read_prop!(dict, "mtu", u16) })
    }
}

/// Read value operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum ReadValueError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Bluetooth operation not authorized")]
    NotAuthorized,
    #[error("Invalid offset for Bluetooth GATT property")]
    InvalidOffset,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<ReadValueError> for dbus::MethodErr {
    fn from(err: ReadValueError) -> Self {
        let name: &'static str = err.clone().into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Write value request.
#[derive(Debug, Clone)]
pub struct WriteCharacteristicValueRequest {
    /// Start offset.
    pub offset: u16,
    /// Write operation type.
    pub op_type: WriteValueType,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: String, // TODO
    /// True if prepare authorization request.
    pub prepare_authorize: bool,
}

impl WriteCharacteristicValueRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self {
            offset: read_prop!(dict, "offset", u16),
            op_type: read_prop!(dict, "op_type", String)
                .parse()
                .map_err(|_| MethodErr::invalid_arg("op_type"))?,
            mtu: read_prop!(dict, "mtu", u16),
            link: read_prop!(dict, "link", String),
            prepare_authorize: read_prop!(dict, "prepare_authorize", bool),
        })
    }
}

/// Write operation type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString, Display)]
pub enum WriteValueType {
    /// Write without response.
    #[strum(serialize = "command")]
    Command,
    /// Write with response.
    #[strum(serialize = "request")]
    Request,
    /// Reliable Write.
    #[strum(serialize = "reliable")]
    Reliable,
}

/// Write value operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum WriteValueError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Invalid value length for Bluetooth GATT property")]
    InvalidValueLength,
    #[error("Bluetooth operation not authorized")]
    NotAuthorized,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<WriteValueError> for dbus::MethodErr {
    fn from(err: WriteValueError) -> Self {
        let name: &'static str = err.clone().into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Notify operation error.
#[derive(Clone, Debug, Error, IntoStaticStr)]
pub enum NotifyError {
    #[error("Bluetooth operation failed")]
    Failed,
    #[error("Bluetooth operation in progress")]
    InProgress,
    #[error("Bluetooth operation not permitted")]
    NotPermitted,
    #[error("Bluetooth device not connected")]
    NotConnected,
    #[error("Bluetooth operation not supported")]
    NotSupported,
}

impl From<NotifyError> for dbus::Error {
    fn from(err: NotifyError) -> Self {
        let name: &'static str = err.clone().into();
        Self::new_custom(ERR_PREFIX.to_string() + name, &err.to_string())
    }
}

/// Local GATT characteristic exposed over Bluetooth.
pub struct Characteristic {
    /// 128-bit characteristic UUID.
    pub uuid: Uuid,
    /// Characteristic flags.
    pub flags: CharacteristicFlags,
    // /// Characteristic descriptors.
    pub descriptors: Vec<CharacteristicDescriptor>,
    /// Read value of characteristic.
    pub read_value: Option<
        Box<
            dyn (Fn(
                    ReadCharacteristicValueRequest,
                ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ReadValueError>> + Send>>)
                + Send
                + Sync,
        >,
    >,
    /// Write value of characteristic.
    pub write_value: Option<
        Box<
            dyn Fn(
                    Vec<u8>,
                    WriteCharacteristicValueRequest,
                ) -> Pin<Box<dyn Future<Output = Result<(), WriteValueError>> + Send>>
                + Send
                + Sync,
        >,
    >,
    // /// Request value change notifications over provided channel.
    // pub notify: Option<Box<dyn Fn(mpsc::Sender<()>) -> Result<(), NotifyError> + Send>>,
    // TODO: file descriptors
    // How to support notification session?
    // Or can't we do that? as a server?
    //
}

define_flags!(CharacteristicFlags, "Local Bluetooth GATT characteristic flags." => {
    broadcast ("broadcast"),
    read ("read"),
    write_without_response ("write-without-response"),
    write ("write"),
    notify ("notify"),
    indicate ("indicate"),
    authenticated_signed_writes ("authenticated-signed-writes"),
    extended_properties ("extended-properties"),
    reliable_write ("reliable-write"),
    writable_auxiliaries ("writable-auxiliaries"),
    encrypt_read ("encrypt-read"),
    encrypt_write ("encrypt-write"),
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    secure_read ("secure-read"),
    secure_write ("secure-write"),
    authorize ("authorize"),
});

fn method_call<
    T: Send + Sync + 'static,
    R: AppendAll,
    F: Future<Output = Result<R, dbus::MethodErr>> + Send + 'static,
>(
    mut ctx: Context, cr: &mut Crossroads, f: impl FnOnce(Arc<T>) -> F,
) -> impl Future<Output = PhantomData<R>> {
    let data_ref: &mut Arc<T> = cr.data_mut(ctx.path()).unwrap();
    let data: Arc<T> = data_ref.clone();
    async move {
        let result = f(data).await;
        ctx.reply(result)
    }
}

impl Characteristic {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattCharacteristic1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", c => {
                Some(c.uuid.to_string())
            });
            cr_property!(ib, "Flags", c => {
                Some(c.flags.to_vec())
            });
            ib.method_with_cr_async("ReadValue", ("options",), ("value",), |ctx, cr, (options,): (PropMap,)| {
                method_call(ctx, cr, |c: Arc<Self>| async move {
                    let options = ReadCharacteristicValueRequest::from_dict(&options)?;
                    match &c.read_value {
                        Some(read_value) => {
                            let value = read_value(options).await?;
                            Ok((value,))
                        }
                        None => Err(ReadValueError::NotSupported.into()),
                    }
                })
            });
            ib.method_with_cr_async(
                "WriteValue",
                ("value", "options"),
                (),
                |ctx, cr, (value, options): (Vec<u8>, PropMap)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = WriteCharacteristicValueRequest::from_dict(&options)?;
                        match &c.write_value {
                            Some(write_value) => {
                                write_value(value, options).await?;
                                Ok(())
                            }
                            None => Err(WriteValueError::NotSupported.into()),
                        }
                    })
                },
            );
        })
    }
}

/// Local GATT characteristic descriptor exposed over Bluetooth.
pub struct CharacteristicDescriptor {
    /// 128-bit descriptor UUID.
    pub uuid: Uuid,
    /// Characteristic descriptor flags.
    pub flags: CharacteristicDescriptorFlags,
    /// Read value of characteristic descriptor.
    pub read_value: Option<
        Box<
            dyn Fn(
                    ReadCharacteristicDescriptorValueRequest,
                ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, ReadValueError>> + Send>>
                + Send
                + Sync,
        >,
    >,
    /// Write value of characteristic descriptor.
    pub write_value: Option<
        Box<
            dyn Fn(
                    Vec<u8>,
                    WriteCharacteristicDescriptorValueRequest,
                ) -> Pin<Box<dyn Future<Output = Result<(), WriteValueError>> + Send>>
                + Send
                + Sync,
        >,
    >,
}

define_flags!(CharacteristicDescriptorFlags, "Local Bluetooth GATT characteristic descriptor flags." => {
    read ("read"),
    write ("write"),
    encrypt_read ("encrypt-read"),
    encrypt_write ("encrypt-write"),
    encrypt_authenticated_read ("encrypt-authenticated-read"),
    encrypt_authenticated_write ("encrypt-authenticated-write"),
    secure_read ("secure-read"),
    secure_write ("secure-write"),
    authorize ("authorize"),
});

/// Read characteristic value request.
#[derive(Debug, Clone)]
pub struct ReadCharacteristicDescriptorValueRequest {
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: String, // TODO
}

impl ReadCharacteristicDescriptorValueRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self { offset: read_prop!(dict, "offset", u16), link: read_prop!(dict, "link", String) })
    }
}

/// Write characteristic value request.
#[derive(Debug, Clone)]
pub struct WriteCharacteristicDescriptorValueRequest {
    /// Offset.
    pub offset: u16,
    /// Link type.
    pub link: String, // TODO
    /// Is prepare authorization request?
    pub prepare_authorize: bool,
}

impl WriteCharacteristicDescriptorValueRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self {
            offset: read_prop!(dict, "offset", u16),
            link: read_prop!(dict, "link", String),
            prepare_authorize: read_prop!(dict, "prepare_authorize", bool),
        })
    }
}

impl CharacteristicDescriptor {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register("org.bluez.GattDescriptor1", |ib: &mut IfaceBuilder<Arc<Self>>| {
            cr_property!(ib, "UUID", cd => {
                Some(cd.uuid.to_string())
            });
            cr_property!(ib, "Flags", cd => {
                Some(cd.flags.to_vec())
            });
            ib.method_with_cr_async("ReadValue", ("flags",), ("value",), |ctx, cr, (flags,): (PropMap,)| {
                method_call(ctx, cr, |c: Arc<Self>| async move {
                    let options = ReadCharacteristicDescriptorValueRequest::from_dict(&flags)?;
                    match &c.read_value {
                        Some(read_value) => {
                            let value = read_value(options).await?;
                            Ok((value,))
                        }
                        None => Err(ReadValueError::NotSupported.into()),
                    }
                })
            });
            ib.method_with_cr_async(
                "WriteValue",
                ("value", "flags"),
                (),
                |ctx, cr, (value, flags): (Vec<u8>, PropMap)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        let options = WriteCharacteristicDescriptorValueRequest::from_dict(&flags)?;
                        match &c.write_value {
                            Some(write_value) => {
                                write_value(value, options).await?;
                                Ok(())
                            }
                            None => Err(WriteValueError::NotSupported.into()),
                        }
                    })
                },
            );
        })
    }
}

/// Local GATT application published over Bluetooth.
pub struct Application {
    pub services: Vec<Service>,
}

impl Application {
    pub(crate) async fn register(
        mut self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ApplicationHandle> {
        let mut reg_paths = Vec::new();
        let app_path = format!("{}{}", GATT_PREFIX, Uuid::new_v4().to_simple());
        let app_path = dbus::Path::new(app_path).unwrap();

        {
            let mut cr = inner.crossroads.lock().await;

            for (service_idx, mut service) in self.services.drain(..).enumerate() {
                let service_path = format!("{}/service{}", &app_path, service_idx);
                for (char_idx, mut char) in service.characteristics.drain(..).enumerate() {
                    let char_path = format!("{}/char{}", &service_path, char_idx);
                    for (desc_idx, desc) in char.descriptors.drain(..).enumerate() {
                        let desc_path = format!("{}/desc{}", &char_path, desc_idx);
                        let desc_path = dbus::Path::new(desc_path).unwrap();
                        reg_paths.push(desc_path.clone());
                        cr.insert(desc_path, &[inner.gatt_characteristic_descriptor_token], Arc::new(desc));
                    }
                    let char_path = dbus::Path::new(char_path).unwrap();
                    reg_paths.push(char_path.clone());
                    cr.insert(char_path, &[inner.gatt_characteristic_token], Arc::new(char));
                }
                let service_path = dbus::Path::new(service_path).unwrap();
                reg_paths.push(service_path.clone());
                cr.insert(service_path, &[inner.gatt_service_token], Arc::new(service));
            }
            reg_paths.push(app_path.clone());
            cr.insert(app_path.clone(), &[], self);
        }

        let proxy =
            Proxy::new(MANAGER_INTERFACE, Adapter::dbus_path(&*adapter_name)?, TIMEOUT, inner.connection.clone());
        proxy.method_call(MANAGER_INTERFACE, "RegisterApplication", (app_path.clone(), PropMap::new())).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let app_path_unreg = app_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;
            let _: std::result::Result<(), dbus::Error> =
                proxy.method_call(MANAGER_INTERFACE, "UnregisterApplication", (app_path_unreg,)).await;

            let mut cr = inner.crossroads.lock().await;
            for reg_path in reg_paths {
                let _: Option<Self> = cr.remove(&reg_path);
            }
        });

        Ok(ApplicationHandle { name: app_path, _drop_tx: drop_tx })
    }
}

pub struct ApplicationHandle {
    name: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for ApplicationHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for ApplicationHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ApplicationHandle {{ {} }}", &self.name)
    }
}
