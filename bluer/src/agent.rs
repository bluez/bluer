//! Bluetooth authorization agent.

use futures::{pin_mut, Future};
use std::{fmt, pin::Pin, sync::Arc};
use strum::IntoStaticStr;
use tokio::{
    select,
    sync::{oneshot, Mutex},
};
use uuid::Uuid;
use zbus::{
    zvariant::{ObjectPath, OwnedObjectPath},
    Proxy,
};

use crate::{Address, Device, Result, SessionInner, ERR_PREFIX, SERVICE_NAME};

pub(crate) const MANAGER_INTERFACE: &str = "org.bluez.AgentManager1";
pub(crate) const MANAGER_PATH: &str = "/org/bluez";
pub(crate) const AGENT_PREFIX: &str = "/org/bluez/agent/";

/// Error response from us to a Bluetooth agent request.
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

impl From<ReqError> for zbus::fdo::Error {
    fn from(err: ReqError) -> Self {
        let name: &'static str = err.into();
        zbus::fdo::Error::Failed(format!("{}.{}", ERR_PREFIX, name))
    }
}

/// Result of a Bluetooth agent request to us.
pub type ReqResult<T> = std::result::Result<T, ReqError>;

/// Arguments for a pin code request.
#[derive(Debug)]
#[non_exhaustive]
pub struct RequestPinCode {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
}

/// Function handling a pin code request.
pub type RequestPinCodeFn =
    Box<dyn (Fn(RequestPinCode) -> Pin<Box<dyn Future<Output = ReqResult<String>> + Send>>) + Send + Sync>;

/// Arguments for a display pin code request.
#[derive(custom_debug::Debug)]
#[non_exhaustive]
pub struct DisplayPinCode {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
    /// Pin code.
    pub pincode: String,
    /// Resolves once the pin code should not be displayed anymore.
    #[debug(skip)]
    pub cancel: oneshot::Receiver<()>,
}

/// Function handling a display pin code request.
pub type DisplayPinCodeFn =
    Box<dyn (Fn(DisplayPinCode) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Argument for a passkey request.
#[derive(Debug)]
#[non_exhaustive]
pub struct RequestPasskey {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
}

/// Function handling a passkey request.
pub type RequestPasskeyFn =
    Box<dyn (Fn(RequestPasskey) -> Pin<Box<dyn Future<Output = ReqResult<u32>> + Send>>) + Send + Sync>;

/// Arguments for a display passkey request.
#[derive(custom_debug::Debug)]
#[non_exhaustive]
pub struct DisplayPasskey {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
    /// Passkey.
    pub passkey: u32,
    /// Digits entered so far.
    pub entered: u16,
    /// Resolves once the passkey should not be displayed anymore.
    #[debug(skip)]
    pub cancel: oneshot::Receiver<()>,
}

/// Function handling a display passkey request.
pub type DisplayPasskeyFn =
    Box<dyn (Fn(DisplayPasskey) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Arguments for a confirmation request.
#[derive(Debug)]
#[non_exhaustive]
pub struct RequestConfirmation {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
    /// Passkey.
    pub passkey: u32,
}

/// Function handling a confirmation request.
pub type RequestConfirmationFn =
    Box<dyn (Fn(RequestConfirmation) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Arguments for an authorization request.
#[derive(Debug)]
#[non_exhaustive]
pub struct RequestAuthorization {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
}

/// Function handling an authorization request.
pub type RequestAuthorizationFn =
    Box<dyn (Fn(RequestAuthorization) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Arguments for an authorize service request.
#[derive(Debug)]
#[non_exhaustive]
pub struct AuthorizeService {
    /// Adapter making the request.
    pub adapter: String,
    /// Address of device making the request.
    pub device: Address,
    /// Service UUID.
    pub service: Uuid,
}

/// Function handling an authorize service request.
pub type AuthorizeServiceFn =
    Box<dyn (Fn(AuthorizeService) -> Pin<Box<dyn Future<Output = ReqResult<()>> + Send>>) + Send + Sync>;

/// Bluetooth authorization agent handler.
///
/// Each handler that is set to [None] will reject the request.
/// The capabilities of the agent are published accordingly.
/// Setting all handlers to [None] (the default) will result in a `NoInputNoOutput` handler
/// that accepts all requests.
///
/// The future of a particular request is dropped when BlueZ cancels that request.
///
/// Use [Session::register_agent](crate::session::Session::register_agent) to register the handler.
#[derive(Default)]
pub struct Agent {
    /// This requests is to make the application agent
    /// the default agent.
    ///
    /// Special permission might be required to become
    /// the default agent.
    pub request_default: bool,
    /// This method gets called when the service daemon
    /// needs to get the passkey for an authentication.
    ///
    /// The return value should be a string of 1-16 characters
    /// length. The string can be alphanumeric.
    pub request_pin_code: Option<RequestPinCodeFn>,
    /// This method gets called when the service daemon
    /// needs to display a pin code for an authentication.
    ///
    /// An empty reply should be returned. When the pin code
    /// needs no longer to be displayed, the Cancel method
    /// of the agent will be called.
    ///
    /// This is used during the pairing process of keyboards
    /// that don't support Bluetooth 2.1 Secure Simple Pairing,
    /// in contrast to DisplayPasskey which is used for those
    /// that do.
    ///
    /// This method will only ever be called once since
    /// older keyboards do not support typing notification.
    ///
    /// Note that the PIN will always be a 6-digit number,
    /// zero-padded to 6 digits. This is for harmony with
    /// the later specification.
    pub display_pin_code: Option<DisplayPinCodeFn>,
    /// This method gets called when the service daemon
    /// needs to get the passkey for an authentication.
    ///
    /// The return value should be a numeric value
    /// between 0-999999.
    pub request_passkey: Option<RequestPasskeyFn>,
    /// This method gets called when the service daemon
    /// needs to display a passkey for an authentication.
    ///
    /// The entered parameter indicates the number of already
    /// typed keys on the remote side.
    ///
    /// An empty reply should be returned. When the passkey
    /// needs no longer to be displayed, the Cancel method
    /// of the agent will be called.
    ///
    /// During the pairing process this method might be
    /// called multiple times to update the entered value.
    ///
    /// Note that the passkey will always be a 6-digit number,
    /// so the display should be zero-padded at the start if
    /// the value contains less than 6 digits.
    pub display_passkey: Option<DisplayPasskeyFn>,
    /// This method gets called when the service daemon
    /// needs to confirm a passkey for an authentication.
    ///
    /// To confirm the value it should return an empty reply
    /// or an error in case the passkey is invalid.
    ///
    /// Note that the passkey will always be a 6-digit number,
    /// so the display should be zero-padded at the start if
    /// the value contains less than 6 digits.
    pub request_confirmation: Option<RequestConfirmationFn>,
    /// This method gets called to request the user to
    /// authorize an incoming pairing attempt which
    /// would in other circumstances trigger the just-works
    /// model, or when the user plugged in a device that
    /// implements cable pairing.
    ///
    /// In the latter case, the
    /// device would not be connected to the adapter via
    /// Bluetooth yet.
    pub request_authorization: Option<RequestAuthorizationFn>,
    /// This method gets called when the service daemon
    /// needs to authorize a connection/service request.
    pub authorize_service: Option<AuthorizeServiceFn>,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Agent {
    /// BlueZ capability parameter.
    pub(crate) fn capability(&self) -> &'static str {
        let keyboard = self.request_passkey.is_some() || self.request_pin_code.is_some();
        let display_only = self.display_passkey.is_some() || self.display_pin_code.is_some();
        let yes_no = self.request_confirmation.is_some()
            || self.request_authorization.is_some()
            || self.authorize_service.is_some();

        match (keyboard, display_only, yes_no) {
            (true, false, false) => "KeyboardOnly",
            (false, true, false) => "DisplayOnly",
            (false, _, true) => "DisplayYesNo",
            (true, true, _) | (true, _, true) => "KeyboardDisplay",
            (false, false, false) => "NoInputNoOutput",
        }
    }
}

pub(crate) struct RegisteredAgent {
    a: Agent,
    cancel: Mutex<Option<oneshot::Sender<()>>>,
}

impl RegisteredAgent {
    pub(crate) fn new(agent: Agent) -> Self {
        Self { a: agent, cancel: Mutex::new(None) }
    }

    async fn get_cancel(&self) -> oneshot::Receiver<()> {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        *self.cancel.lock().await = Some(cancel_tx);
        cancel_rx
    }

    async fn call<A, F, R>(&self, f: &Option<impl Fn(A) -> F>, arg: A) -> ReqResult<R>
    where
        F: Future<Output = ReqResult<R>> + Send + 'static,
    {
        match f {
            Some(f) => f(arg).await,
            None => Err(ReqError::Rejected),
        }
    }

    async fn call_with_cancel<A, F, R>(&self, f: &Option<impl Fn(A) -> F>, arg: A) -> ReqResult<R>
    where
        F: Future<Output = ReqResult<R>> + Send + 'static,
    {
        let cancel_rx = self.get_cancel().await;
        match f {
            Some(f) => {
                let fut = f(arg);
                pin_mut!(fut);
                select! {
                    result = fut => result,
                    _ = cancel_rx => Err(ReqError::Canceled)
                }
            }
            None => Err(ReqError::Rejected),
        }
    }

    fn parse_device_path(device: &ObjectPath) -> ReqResult<(String, Address)> {
        match Device::parse_dbus_path(device) {
            Some((adapter, addr)) => Ok((adapter.to_string(), addr)),
            None => {
                log::error!("Cannot parse device path {}", &device);
                Err(ReqError::Rejected)
            }
        }
    }

    pub(crate) async fn register(self, inner: Arc<SessionInner>) -> Result<AgentHandle> {
        let name = OwnedObjectPath::try_from(format!("{}{}", AGENT_PREFIX, Uuid::new_v4().as_simple())).unwrap();
        let capability = self.a.capability();
        let request_default = self.a.request_default;
        log::trace!("Publishing agent at {} with capability {}", &name, &capability);

        inner.connection.object_server().at(name.clone(), self).await?;

        log::trace!("Registering agent at {}", &name);
        let proxy = Proxy::new(&inner.connection, SERVICE_NAME, MANAGER_PATH, MANAGER_INTERFACE).await?;
        let () = proxy.call("RegisterAgent", &(name.clone(), capability)).await?;

        let (drop_tx, drop_rx) = oneshot::channel();
        let unreg_name = name.clone();
        let connection = inner.connection.clone();
        tokio::spawn(async move {
            let _ = drop_rx.await;

            log::trace!("Unregistering agent at {}", &unreg_name);
            let proxy = Proxy::new(&connection, SERVICE_NAME, MANAGER_PATH, MANAGER_INTERFACE).await;
            if let Ok(proxy) = proxy {
                let _: std::result::Result<(), zbus::Error> =
                    proxy.call("UnregisterAgent", &(unreg_name.clone(),)).await;
            }

            log::trace!("Unpublishing agent at {}", &unreg_name);
            let _ = connection.object_server().remove::<RegisteredAgent, _>(&unreg_name).await;
        });

        if request_default {
            log::trace!("Requesting default agent for {}", &name);
            let () = proxy.call("RequestDefaultAgent", &(name.clone(),)).await?;
        }

        Ok(AgentHandle { name, _drop_tx: drop_tx })
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl RegisteredAgent {
    async fn release(&self) {
        log::trace!("Agent released");
    }

    async fn cancel(&self) -> zbus::fdo::Result<()> {
        if let Some(cancel_tx) = self.cancel.lock().await.take() {
            let _ = cancel_tx.send(());
        }
        Ok(())
    }

    async fn request_pin_code(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<String> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call_with_cancel(&self.a.request_pin_code, RequestPinCode { adapter, device })
            .await
            .map_err(Into::into)
    }

    async fn display_pin_code(&self, device: ObjectPath<'_>, pincode: String) -> zbus::fdo::Result<()> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call(
            &self.a.display_pin_code,
            DisplayPinCode { adapter, device, pincode, cancel: self.get_cancel().await },
        )
        .await
        .map_err(Into::into)
    }

    async fn request_passkey(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<u32> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call_with_cancel(&self.a.request_passkey, RequestPasskey { adapter, device })
            .await
            .map_err(Into::into)
    }

    async fn display_passkey(&self, device: ObjectPath<'_>, passkey: u32, entered: u16) -> zbus::fdo::Result<()> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call(
            &self.a.display_passkey,
            DisplayPasskey { adapter, device, passkey, entered, cancel: self.get_cancel().await },
        )
        .await
        .map_err(Into::into)
    }

    async fn request_confirmation(&self, device: ObjectPath<'_>, passkey: u32) -> zbus::fdo::Result<()> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call_with_cancel(&self.a.request_confirmation, RequestConfirmation { adapter, device, passkey })
            .await
            .map_err(Into::into)
    }

    async fn request_authorization(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<()> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.call_with_cancel(&self.a.request_authorization, RequestAuthorization { adapter, device })
            .await
            .map_err(Into::into)
    }

    async fn authorize_service(&self, device: ObjectPath<'_>, uuid: String) -> zbus::fdo::Result<()> {
        let (adapter, device) =
            Self::parse_device_path(&device).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let service: Uuid = match uuid.parse() {
            Ok(service) => service,
            Err(_) => {
                log::error!("Invalid UUID: {}", &uuid);
                return Err(zbus::fdo::Error::Failed("Invalid UUID".to_string()));
            }
        };
        self.call_with_cancel(&self.a.authorize_service, AuthorizeService { adapter, device, service })
            .await
            .map_err(Into::into)
    }
}

/// Handle to registered agent.
///
/// Drop to unregister agent.
#[must_use = "AgentHandle must be held for agent to be registered"]
pub struct AgentHandle {
    name: OwnedObjectPath,
    _drop_tx: oneshot::Sender<()>,
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        // required for drop order
    }
}

impl fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AgentHandle {{ {} }}", &self.name)
    }
}
