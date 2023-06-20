//! Bluetooth mesh element.

use dbus::{
    arg::{ArgType, RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};
use futures::{Stream, StreamExt};
use std::{
    collections::HashMap,
    fmt,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    mesh::{ReqError, PATH, SERVICE_NAME, TIMEOUT},
    method_call, Error, ErrorKind, Result, SessionInner,
};

pub(crate) const ELEMENT_INTERFACE: &str = "org.bluez.mesh.Element1";

pub(crate) type ElementConfig = HashMap<String, Variant<Box<dyn RefArg + 'static>>>;
pub(crate) type ElementConfigs = HashMap<usize, HashMap<u16, ElementConfig>>;

/// Interface to a Bluetooth mesh element interface.
#[derive(Debug, Clone, Default)]
pub struct Element {
    /// Location descriptor as defined in the GATT Bluetooth Namespace
    /// Descriptors section of the Bluetooth SIG Assigned Numbers.
    pub location: Option<u16>,
    /// Element SIG models.
    pub models: Vec<Model>,
    /// Vendor models.
    pub vendor_models: Vec<VendorModel>,
    /// Control handle for element once it has been registered.
    pub control_handle: ElementControlHandle,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

/// SIG model information.
#[derive(Debug, Clone)]
pub struct Model {
    /// SIG model identifier.
    pub id: u16,
    /// Indicates whether the model supports publication mechanism.
    ///
    /// By default this is true.
    pub publish: bool,
    /// Indicates whether the model supports subscription mechanism.
    ///
    /// By default this is true.
    pub subscribe: bool,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Model {
    /// Creates a new model with the specified SIG model identifier.
    pub fn new(id: u16) -> Self {
        Self { id, ..Default::default() }
    }

    fn as_tuple(&self) -> (u16, HashMap<String, Variant<Box<dyn RefArg>>>) {
        let mut opts: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        opts.insert("Publish".to_string(), Variant(Box::new(self.publish)));
        opts.insert("Subscribe".to_string(), Variant(Box::new(self.subscribe)));
        (self.id, opts)
    }
}

impl Default for Model {
    fn default() -> Self {
        Self { id: 0, publish: true, subscribe: true, _non_exhaustive: Default::default() }
    }
}

/// Vendor model information.
#[derive(Debug, Clone)]
pub struct VendorModel {
    /// Company id.
    pub vendor: u16,
    /// Vendor-assigned model identifier.
    pub id: u16,
    /// Indicates whether the model supports publication mechanism.
    ///
    /// By default this is true.
    pub publish: bool,
    /// Indicates whether the model supports subscription mechanism.
    ///
    /// By default this is true.
    pub subscribe: bool,
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl VendorModel {
    /// Creates a new model with the vendor and model identifiers.
    pub fn new(vendor: u16, id: u16) -> Self {
        Self { vendor, id, ..Default::default() }
    }

    #[allow(clippy::type_complexity)]
    fn as_tuple(&self) -> (u16, u16, HashMap<String, Variant<Box<dyn RefArg>>>) {
        let mut opts: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        opts.insert("Publish".to_string(), Variant(Box::new(self.publish)));
        opts.insert("Subscribe".to_string(), Variant(Box::new(self.subscribe)));
        (self.vendor, self.id, opts)
    }
}

impl Default for VendorModel {
    fn default() -> Self {
        Self { vendor: 0, id: 0, publish: true, subscribe: true, _non_exhaustive: Default::default() }
    }
}

/// An element exposed over D-Bus to bluez.
pub(crate) struct RegisteredElement {
    inner: Arc<SessionInner>,
    element: Element,
    index: usize,
}

impl RegisteredElement {
    pub(crate) fn new(inner: Arc<SessionInner>, root_path: String, element: Element, index: usize) -> Self {
        *element.control_handle.element_ref.lock().unwrap() = Some(ElementRefInner { root_path, index });
        Self { inner, element, index }
    }

    fn proxy(&self) -> Proxy<'_, &SyncConnection> {
        Proxy::new(SERVICE_NAME, PATH, TIMEOUT, &*self.inner.connection)
    }

    dbus_interface!();
    dbus_default_interface!(ELEMENT_INTERFACE);

    pub(crate) fn register_interface(cr: &mut Crossroads) -> IfaceToken<Arc<Self>> {
        cr.register(ELEMENT_INTERFACE, |ib: &mut IfaceBuilder<Arc<Self>>| {
            ib.method_with_cr_async(
                "MessageReceived",
                ("source", "key_index", "destination", "data"),
                (),
                |ctx,
                 cr,
                 (source, key_index, destination, data): (
                    u16,
                    u16,
                    Variant<Box<dyn RefArg + 'static>>,
                    Vec<u8>,
                )| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        log::trace!(
                            "Message received for element {:?}: source={:?} key_index={:?} dest={:?} data={:?}",
                            reg.index,
                            source,
                            key_index,
                            destination,
                            data
                        );

                        let destination = match destination.0.arg_type() {
                            ArgType::Array => {
                                let args = dbus::arg::cast::<Vec<u8>>(&destination.0).ok_or(ReqError::Failed)?;
                                if args.len() < 2 {
                                    return Err(ReqError::Failed.into());
                                }
                                u16::from_be_bytes([args[0], args[1]])
                            }
                            ArgType::UInt16 => *dbus::arg::cast::<u16>(&destination.0).ok_or(ReqError::Failed)?,
                            _ => return Err(ReqError::Failed.into()),
                        };

                        let msg = ReceivedMessage {
                            key_index,
                            source,
                            destination,
                            data,
                        };
                        reg.element.control_handle
                            .event_tx
                            .send(ElementEvent::MessageReceived(msg))
                            .await
                            .map_err(|_| ReqError::Failed)?;

                        Ok(())
                    })
                },
            );

            ib.method_with_cr_async(
                "DevKeyMessageReceived",
                ("source", "remote", "net_index", "data"),
                (),
                |ctx,
                 cr,
                 (source, remote, net_index, data): (
                    u16,
                    bool,
                    u16,
                    Vec<u8>,
                )| {
                    method_call(ctx, cr, move |reg: Arc<Self>| async move {
                        log::trace!(
                            "Dev Key Message received for element {:?}: source={:?} net_index={:?} remote={:?} data={:?}",
                            reg.index,
                            source,
                            net_index,
                            remote,
                            data
                        );

                        let msg = ReceivedDevKeyMessage {
                            source,
                            remote,
                            net_index,
                            data,
                        };
                        reg.element.control_handle
                            .event_tx
                            .send(ElementEvent::DevKeyMessageReceived(msg))
                            .await
                            .map_err(|_| ReqError::Failed)?;

                        Ok(())
                    })
                },
            );

            cr_property!(ib, "Index", reg => {
                Some(reg.index as u8)
            });

            cr_property!(ib, "Models", reg => {
                Some(reg.element.models.iter().map(|m| m.as_tuple()).collect::<Vec<_>>())
            });

            cr_property!(ib, "VendorModels", reg => {
                Some(reg.element.vendor_models.iter().map(|m| m.as_tuple()).collect::<Vec<_>>())
            });

            cr_property!(ib, "Location", reg => {
                reg.element.location
            });
        })
    }
}

/// A reference to a registered element.
#[derive(Clone)]
pub struct ElementRef(Weak<std::sync::Mutex<Option<ElementRefInner>>>);

impl ElementRef {
    /// Element index.
    ///
    /// `None` if the element is currently not registered.
    pub fn index(&self) -> Option<usize> {
        self.0.upgrade().and_then(|m| m.lock().unwrap().as_ref().map(|i| i.index))
    }

    /// Element D-Bus path.
    pub(crate) fn path(&self) -> Result<dbus::Path<'static>> {
        self.0
            .upgrade()
            .and_then(|m| m.lock().unwrap().as_ref().map(|i| i.path()))
            .ok_or_else(|| Error::new(ErrorKind::MeshElementUnpublished))
    }
}

impl fmt::Debug for ElementRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ElementRef").field("index", &self.index()).finish()
    }
}

struct ElementRefInner {
    root_path: String,
    index: usize,
}

impl ElementRefInner {
    /// Element D-Bus path.
    fn path(&self) -> dbus::Path<'static> {
        let element_path = format!("{}/ele{}", &self.root_path, self.index);
        dbus::Path::new(element_path).unwrap()
    }
}

/// An object to control an element and receive events once it has been registered.
///
/// Use [element_control] to obtain controller and associated handle.
pub struct ElementControl {
    event_rx: ReceiverStream<ElementEvent>,
    element_ref: ElementRef,
}

impl fmt::Debug for ElementControl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ElementControl").finish()
    }
}

impl ElementControl {
    /// Returns a reference to the registered element.
    pub fn element_ref(&self) -> ElementRef {
        self.element_ref.clone()
    }
}

impl Stream for ElementControl {
    type Item = ElementEvent;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::into_inner(self).event_rx.poll_next_unpin(cx)
    }
}

/// A handle to store inside a element definition to make it controllable
/// once it has been registered.
///
/// Use [element_control] to obtain controller and associated handle.
#[derive(Clone)]
pub struct ElementControlHandle {
    event_tx: mpsc::Sender<ElementEvent>,
    element_ref: Arc<std::sync::Mutex<Option<ElementRefInner>>>,
}

impl Default for ElementControlHandle {
    fn default() -> Self {
        Self { event_tx: mpsc::channel(1).0, element_ref: Arc::new(std::sync::Mutex::new(None)) }
    }
}

impl fmt::Debug for ElementControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ElementControlHandle").finish()
    }
}

/// Creates a [ElementControl] and its associated [ElementControlHandle].
///
/// Keep the [ElementControl] and store the [ElementControlHandle] in [Element::control_handle].
pub fn element_control() -> (ElementControl, ElementControlHandle) {
    let (event_tx, event_rx) = mpsc::channel(128);
    let inner = Arc::new(std::sync::Mutex::new(None));
    (
        ElementControl {
            event_rx: ReceiverStream::new(event_rx),
            element_ref: ElementRef(Arc::downgrade(&inner)),
        },
        ElementControlHandle { event_tx, element_ref: inner },
    )
}

/// Bluetooth mesh element events received by the application.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ElementEvent {
    /// A message arrived addressed to the application element.
    MessageReceived(ReceivedMessage),
    /// A message arrived addressed to the application element,
    /// which was sent with the remote node's device key.
    DevKeyMessageReceived(ReceivedDevKeyMessage),
}

/// A message addressed to the application element.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReceivedMessage {
    /// Index of application key used to decode the incoming message.
    ///
    /// The same key_index should
    /// be used by the application when sending a response to this
    /// message (in case a response is expected).
    pub key_index: u16,
    /// Unicast address of the remote node-element that sent the message.
    pub source: u16,
    /// The destination address of the received message.
    pub destination: u16,
    /// Incoming message.
    pub data: Vec<u8>,
}

/// Message originated by a local model encoded with the device key of the remote node.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReceivedDevKeyMessage {
    /// Unicast address of the remote node-element that sent the message.
    pub source: u16,
    /// Device key remote origin.
    ///
    /// The remote parameter if true indicates that the device key
    /// used to decrypt the message was from the sender.
    /// False indicates that the local nodes device key was used, and the
    /// message has permissions to modify local states.
    pub remote: bool,
    /// Subnet message was received on.
    ///
    /// The net_index parameter indicates what subnet the message was
    /// received on, and if a response is required, the same subnet
    /// must be used to send the response.
    pub net_index: u16,
    /// Incoming message.
    pub data: Vec<u8>,
}
