//! Implement Element bluetooth mesh interface

use crate::{method_call, SessionInner};
use btmesh_common::{
    address::{Address, UnicastAddress},
    crypto::application::Aid,
    opcode::Opcode,
    ModelIdentifier,
};
use dbus::{
    arg::{ArgType, RefArg, Variant},
    nonblock::{Proxy, SyncConnection},
};
use dbus_crossroads::{Crossroads, IfaceBuilder, IfaceToken};

use crate::mesh::{ReqError, PATH, SERVICE_NAME, TIMEOUT};
use futures::Stream;
use pin_project::pin_project;
use std::{collections::HashMap, fmt, pin::Pin, sync::Arc, task::Poll};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub(crate) const ELEMENT_INTERFACE: &str = "org.bluez.mesh.Element1";

pub(crate) type ElementConfig = HashMap<String, Variant<Box<dyn RefArg + 'static>>>;

/// Interface to a Bluetooth mesh element interface.
#[derive(Debug, Clone)]
pub struct Element {
    /// Element location
    pub location: Option<u16>,
    /// Element models
    pub models: Vec<ModelIdentifier>,
    /// Control handle for element once it has been registered.
    pub control_handle: Option<ElementControlHandle>,
}

/// An element exposed over D-Bus to bluez.
pub struct RegisteredElement {
    inner: Arc<SessionInner>,
    element: Element,
    index: u8,
}

impl RegisteredElement {
    pub(crate) fn new(inner: Arc<SessionInner>, element: Element, index: u8) -> Self {
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
                            "Message received for element {:?}: (source: {:?}, key_index: {:?}, dest: {:?}, data: {:?})",
                            reg.index,
                            source,
                            key_index,
                            destination,
                            data
                        );

                        let key = Aid::from(u8::try_from(key_index).unwrap_or_default());
                        let src: UnicastAddress = source.try_into().map_err(|_| ReqError::Failed)?;
                        let value = &destination.0;
                        let dest = match value.arg_type() {
                            ArgType::Array => {
                                let args = dbus::arg::cast::<Vec<u8>>(value).unwrap();
                                assert!(args.len() >= 2);
                                Ok(Address::parse([args[0], args[1]]))
                            }
                            ArgType::UInt16 => {
                                Ok(Address::parse(dbus::arg::cast::<u16>(value).unwrap().to_be_bytes()))
                            }
                            _ => Err(ReqError::Failed),
                        }?;

                        let (opcode, parameters) = Opcode::split(&data[..]).unwrap();
                        let parameters = parameters.to_vec();
                        let index = reg.index;
                        let location: Option<u16> = reg.element.location;
                        let msg = ReceivedMessage {
                            index, location, key, src, dest, opcode, parameters
                        };

                        match &reg.element.control_handle {
                            Some(handler) => {
                                handler
                                .messages_tx
                                .send(ElementMessage::Received(msg))
                                .await
                                .map_err(|_| ReqError::Failed)?;
                            }
                            None => ()
                        }

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
                            "Dev Key Message received for element {:?}: (source: {:?}, net_index: {:?}, remote: {:?}, data: {:?})",
                            reg.index,
                            source,
                            net_index,
                            remote,
                            data
                        );

                        let (opcode, parameters) = Opcode::split(&data[..]).unwrap();
                        let parameters = parameters.to_vec();

                        let msg = DevKeyMessage {
                            opcode, parameters
                        };

                        match &reg.element.control_handle {
                            Some(handler) => {
                                handler
                                .messages_tx
                                .send(ElementMessage::DevKey(msg))
                                .await
                                .map_err(|_| ReqError::Failed)?;
                            }
                            None => ()
                        }

                        Ok(())
                    })
                },
            );
            cr_property!(ib, "Index", reg => {
                Some(reg.index)
            });
            cr_property!(ib, "Models", reg => {
                let mut mt: Vec<(u16, ElementConfig)> = vec![];
                for model in &reg.element.models {
                    if let ModelIdentifier::SIG(id) = model {
                        mt.push((*id, HashMap::new()));
                    }
                }
                Some(mt)
            });
            cr_property!(ib, "VendorModels", reg => {
                let mut mt: Vec<(u16, u16, ElementConfig)> = vec![];
                for model in &reg.element.models {
                    if let ModelIdentifier::Vendor(vid, id) = model {
                        mt.push((vid.0, *id, HashMap::new()));
                    }
                }
                Some(mt)
            });
            cr_property!(ib, "Location", reg => {
                reg.element.location
            });
        })
    }
}

/// An object to control a element and receive messages once it has been registered.
///
/// Use [element_control] to obtain controller and associated handle.
#[pin_project]
pub struct ElementControl {
    #[pin]
    messages_rx: ReceiverStream<ElementMessage>,
}

impl Stream for ElementControl {
    type Item = ElementMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut std::task::Context) -> Poll<Option<Self::Item>> {
        self.project().messages_rx.poll_next(cx)
    }
}

/// A handle to store inside a element definition to make it controllable
/// once it has been registered.
///
/// Use [element_control] to obtain controller and associated handle.
#[derive(Clone)]
pub struct ElementControlHandle {
    messages_tx: mpsc::Sender<ElementMessage>,
}

impl Default for ElementControlHandle {
    fn default() -> Self {
        Self { messages_tx: mpsc::channel(1).0 }
    }
}

impl fmt::Debug for ElementControlHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ElementControlHandle")
    }
}

/// Creates a [ElementControl] and its associated [ElementControlHandle].
///
/// Keep the [ElementControl] and store the [ElementControlHandle] in [Element::control_handle].
pub fn element_control(size: usize) -> (ElementControl, ElementControlHandle) {
    let (messages_tx, messages_rx) = mpsc::channel(size);
    (ElementControl { messages_rx: ReceiverStream::new(messages_rx) }, ElementControlHandle { messages_tx })
}

#[derive(Clone, Debug)]
///Messages sent by the element.
pub enum ElementMessage {
    /// Received Message
    Received(ReceivedMessage),
    /// DevKey Message
    DevKey(DevKeyMessage),
}

/// Element message received from dbus
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    /// Index
    pub index: u8,
    /// Location
    pub location: Option<u16>,
    /// Application key
    pub key: Aid,
    /// Message source
    pub src: UnicastAddress,
    /// Message destination
    pub dest: Address,
    /// Message opcode
    pub opcode: Opcode,
    /// Message data
    pub parameters: Vec<u8>,
}

/// DevKey message
#[derive(Clone, Debug)]
pub struct DevKeyMessage {
    /// Message opcode
    pub opcode: Opcode,
    /// Message data
    pub parameters: Vec<u8>,
}
