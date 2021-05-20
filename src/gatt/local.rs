//! Local GATT services.

use dbus::{
    arg::{AppendAll, OwnedFd, PropMap},
    nonblock::Proxy,
    MethodErr,
};
use dbus_crossroads::{Context, Crossroads, IfaceBuilder, IfaceToken};
use futures::{channel::oneshot, Future};
use libc::{c_int, socketpair, AF_LOCAL, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_SEQPACKET};
use std::{
    collections::HashSet,
    fmt,
    marker::PhantomData,
    mem::take,
    os::unix::{io::RawFd, prelude::FromRawFd},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use strum::IntoStaticStr;
use thiserror::Error;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::sleep,
};
use uuid::Uuid;

use super::{CharacteristicDescriptorFlags, CharacteristicFlags, WriteValueType};
use crate::{Adapter, SessionInner, ERR_PREFIX, SERVICE_NAME, TIMEOUT};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.GattManager1";

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

// ===========================================================================================
// Service
// ===========================================================================================

/// Local GATT service exposed over Bluetooth.
#[derive(Debug)]
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

// ===========================================================================================
// Characteristic
// ===========================================================================================

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

impl fmt::Debug for Characteristic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Characteristic {{ uuid: {:?}, flags: {:?}, descriptors: {:?}, read_value: {:?}, write_value: {:?} }}",
            &self.uuid, &self.flags, &self.descriptors, self.read_value.is_some(), self.write_value.is_some())
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
            ib.property("Service").get(|ctx, _| {
                let mut comps: Vec<_> = ctx.path().split('/').collect();
                comps.pop();
                let service_path = dbus::Path::new(comps.join("/")).unwrap();
                dbg!(&service_path);
                Ok(service_path)
            });
            cr_property!(ib, "WriteAcquired", _c => {
                Some(false)
            });
            cr_property!(ib, "NotifyAcquired", _c => {
                Some(false)
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
                        //dbg!(&options);
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
            ib.method_with_cr_async(
                "AcquireWrite",
                ("options",),
                ("fd", "mtu"),
                |ctx, cr, (options,): (PropMap,)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        dbg!(&options);
                        let options = AcquireWriteRequest::from_dict(&options)?;

                        let (fd, mut us) = make_socket_pair().map_err(|_| AcquireWriteError::Failed)?;
                        let mtu = options.mtu;
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; mtu as _];
                            loop {
                                match us.read(&mut buf).await {
                                    Ok(n) if n == 0 => {
                                        eprintln!("socket ended");
                                        break;
                                    }
                                    Ok(n) => {
                                        eprintln!("read: {:?}", &buf[0..n]);
                                    }
                                    Err(err) => {
                                        eprintln!("socket read fialed: {}", &err);
                                    }
                                }
                            }
                        });

                        Ok((fd, options.mtu))
                    })
                },
            );
            ib.method_with_cr_async(
                "AcquireNotify",
                ("options",),
                ("fd", "mtu"),
                |ctx, cr, (options,): (PropMap,)| {
                    method_call(ctx, cr, |c: Arc<Self>| async move {
                        dbg!(&options);
                        let options = AcquireNotifyRequest::from_dict(&options)?;

                        let (fd, mut us) = make_socket_pair().map_err(|_| AcquireWriteError::Failed)?;
                        let mtu = options.mtu;
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; mtu as _];
                            for i in 0..buf.len() {
                                buf[i] = (i % u8::MAX as usize) as u8;
                            }
                            loop {
                                match us.write(&buf).await {
                                    Ok(n) => {
                                        eprintln!("Notified {} bytes", n);
                                    }
                                    Err(err) => {
                                        eprintln!("notify write failed: {}", &err);
                                        break;
                                    }
                                }
                                sleep(Duration::from_secs(10)).await;
                            }
                        });
                        Ok((fd, options.mtu))
                    })
                },
            );
        })
    }
}

fn make_socket_pair() -> Result<(OwnedFd, UnixStream), io::Error> {
    let mut sv: [RawFd; 2] = [0; 2];
    unsafe {
        if socketpair(AF_LOCAL, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, &mut sv as *mut c_int) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    let [fd1, fd2] = sv;

    let fd1 = unsafe { OwnedFd::new(fd1) };
    let us = UnixStream::from_std(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd2) })?;

    Ok((fd1, us))
}

/// Read value request.
#[derive(Debug, Clone)]
pub struct ReadCharacteristicValueRequest {
    /// Offset.
    pub offset: u16,
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: String,
}

impl ReadCharacteristicValueRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self {
            offset: read_opt_prop!(dict, "offset", u16).unwrap_or_default(),
            mtu: read_prop!(dict, "mtu", u16),
            link: read_prop!(dict, "link", String),
        })
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
            offset: read_opt_prop!(dict, "offset", u16).unwrap_or_default(),
            op_type: read_opt_prop!(dict, "type", String)
                .map(|s| s.parse().map_err(|_| MethodErr::invalid_arg("type")))
                .transpose()?
                .unwrap_or_default(),
            mtu: read_prop!(dict, "mtu", u16),
            link: read_prop!(dict, "link", String),
            prepare_authorize: read_opt_prop!(dict, "prepare-authorize", bool).unwrap_or_default(),
        })
    }
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

/// Acquire write request.
#[derive(Debug, Clone)]
struct AcquireWriteRequest {
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: String, // TODO
}

impl AcquireWriteRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self { mtu: read_prop!(dict, "mtu", u16), link: read_prop!(dict, "link", String) })
    }
}

#[derive(Clone, Debug, Error, IntoStaticStr)]
enum AcquireWriteError {
    #[error("Failed")]
    Failed,
    #[error("Not supported")]
    NotSupported,
}

impl From<AcquireWriteError> for dbus::MethodErr {
    fn from(err: AcquireWriteError) -> Self {
        let name: &'static str = err.clone().into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

/// Acquire write request.
#[derive(Debug, Clone)]
struct AcquireNotifyRequest {
    /// Exchanged MTU.
    pub mtu: u16,
    /// Link type.
    pub link: String, // TODO
}

impl AcquireNotifyRequest {
    fn from_dict(dict: &PropMap) -> Result<Self, dbus::MethodErr> {
        Ok(Self { mtu: read_prop!(dict, "mtu", u16), link: read_prop!(dict, "link", String) })
    }
}

#[derive(Clone, Debug, Error, IntoStaticStr)]
enum AcquireNotifyError {
    #[error("Failed")]
    Failed,
    #[error("Not supported")]
    NotSupported,
}

impl From<AcquireNotifyError> for dbus::MethodErr {
    fn from(err: AcquireNotifyError) -> Self {
        let name: &'static str = err.clone().into();
        Self::from((ERR_PREFIX.to_string() + name, &err.to_string()))
    }
}

// ===========================================================================================
// Characteristic descriptor
// ===========================================================================================

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

impl fmt::Debug for CharacteristicDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CharacteristicDescriptor {{ uuid: {:?}, flags: {:?}, read_value: {:?}, write_value: {:?} }}",
            &self.uuid,
            &self.flags,
            self.read_value.is_some(),
            self.write_value.is_some()
        )
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
            ib.property("Characteristic").get(|ctx, _| {
                let mut comps: Vec<_> = ctx.path().split('/').collect();
                comps.pop();
                let char_path = dbus::Path::new(comps.join("/")).unwrap();
                dbg!(&char_path);
                Ok(char_path)
            });
            ib.method_with_cr_async("ReadValue", ("flags",), ("value",), |ctx, cr, (flags,): (PropMap,)| {
                method_call(ctx, cr, |c: Arc<Self>| async move {
                    dbg!(&flags);
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

// ===========================================================================================
// Application
// ===========================================================================================

pub(crate) const GATT_APP_PREFIX: &str = "/io/crates/tokio_bluez/gatt/app/";

/// Local GATT application to publish over Bluetooth.
#[derive(Debug)]
pub struct Application {
    /// Services to publish.
    pub services: Vec<Service>,
}

impl Application {
    pub(crate) async fn register(
        mut self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ApplicationHandle> {
        let mut reg_paths = Vec::new();
        let app_path = format!("{}{}", GATT_APP_PREFIX, Uuid::new_v4().to_simple());
        let app_path = dbus::Path::new(app_path).unwrap();

        {
            let mut cr = inner.crossroads.lock().await;

            let services = take(&mut self.services);
            reg_paths.push(app_path.clone());
            let om = cr.object_manager::<Self>();
            cr.insert(app_path.clone(), &[om], self);

            for (service_idx, mut service) in services.into_iter().enumerate() {
                let chars = take(&mut service.characteristics);
                let service_path = format!("{}/service{}", &app_path, service_idx);
                let service_path = dbus::Path::new(service_path).unwrap();
                reg_paths.push(service_path.clone());
                cr.insert(service_path.clone(), &[inner.gatt_service_token], Arc::new(service));

                for (char_idx, mut char) in chars.into_iter().enumerate() {
                    let descs = take(&mut char.descriptors);

                    let char_path = format!("{}/char{}", &service_path, char_idx);
                    let char_path = dbus::Path::new(char_path).unwrap();
                    reg_paths.push(char_path.clone());
                    cr.insert(char_path.clone(), &[inner.gatt_characteristic_token], Arc::new(char));

                    for (desc_idx, desc) in descs.into_iter().enumerate() {
                        let desc_path = format!("{}/desc{}", &char_path, desc_idx);
                        let desc_path = dbus::Path::new(desc_path).unwrap();
                        reg_paths.push(desc_path.clone());
                        cr.insert(desc_path, &[inner.gatt_characteristic_descriptor_token], Arc::new(desc));
                    }
                }
            }
        }

        let proxy =
            Proxy::new(SERVICE_NAME, Adapter::dbus_path(&*adapter_name)?, TIMEOUT, inner.connection.clone());
        dbg!(&app_path);
        //future::pending::<()>().await;
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

/// Local GATT application published over Bluetooth.
///
/// Drop this handle to unpublish.
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

// ===========================================================================================
// GATT profile
// ===========================================================================================

pub(crate) const GATT_PROFILE_PREFIX: &str = "/io/crates/tokio_bluez/gatt/profile/";

/// Local profile (GATT client) instance.
///
/// By registering this type of object
/// an application effectively indicates support for a specific GATT profile
/// and requests automatic connections to be established to devices
/// supporting it.
#[derive(Debug, Clone)]
pub struct Profile {
    /// 128-bit GATT service UUIDs to auto connect.
    pub uuids: HashSet<Uuid>,
}

impl Profile {
    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Self> {
        cr.register("org.bluez.GattProfile1", |ib: &mut IfaceBuilder<Self>| {
            cr_property!(ib, "UUIDs", p => {
                Some(p.uuids.iter().map(|uuid| uuid.to_string()).collect::<Vec<_>>())
            });
        })
    }

    pub(crate) async fn register(
        self, inner: Arc<SessionInner>, adapter_name: Arc<String>,
    ) -> crate::Result<ProfileHandle> {
        let profile_path = format!("{}{}", GATT_PROFILE_PREFIX, Uuid::new_v4().to_simple());
        let profile_path = dbus::Path::new(profile_path).unwrap();

        {
            let mut cr = inner.crossroads.lock().await;
            let om = cr.object_manager::<Self>();
            cr.insert(profile_path.clone(), &[inner.gatt_profile_token, om], self);
        }

        let proxy =
            Proxy::new(SERVICE_NAME, Adapter::dbus_path(&*adapter_name)?, TIMEOUT, inner.connection.clone());
        dbg!(&profile_path);
        //future::pending::<()>().await;
        proxy
            .method_call(MANAGER_INTERFACE, "RegisterApplication", (profile_path.clone(), PropMap::new()))
            .await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let profile_path_unreg = profile_path.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            let _: std::result::Result<(), dbus::Error> = proxy
                .method_call(MANAGER_INTERFACE, "UnregisterApplication", (profile_path_unreg.clone(),))
                .await;

            let mut cr = inner.crossroads.lock().await;
            let _: Option<Self> = cr.remove(&profile_path_unreg);
        });

        Ok(ProfileHandle { name: profile_path, _drop_tx: drop_tx })
    }
}

/// Published local profile (GATT client) instance.
///
/// Drop this handle to unpublish.
pub struct ProfileHandle {
    name: dbus::Path<'static>,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for ProfileHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for ProfileHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProfileHandle {{ {} }}", &self.name)
    }
}
