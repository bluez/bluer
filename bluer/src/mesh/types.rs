pub use btmesh_common::{
    address::{Address, UnicastAddress},
    InsufficientBuffer, ModelIdentifier,
};
use btmesh_common::{opcode::Opcode, ParseError};
use btmesh_models::{Message as ConcreteMessage, Model as ConcreteModel};
use core::fmt::Debug;

/// Bluetooth Mesh Message
pub trait Message {
    /// Returns opcode of the message
    fn opcode(&self) -> Opcode;
    /// Emit message parameters
    fn emit_parameters(&self, xmit: &mut Vec<u8>);
}

/// Bluetooth Mesh Model
pub trait Model: Sync + Send + Debug {
    /// Returns model identifier
    fn identifier(&self) -> ModelIdentifier;
    /// Returns whether model supports subscription
    fn supports_subscription(&self) -> bool;
    /// Returns whether model supports publication
    fn supports_publication(&self) -> bool;

    /// Parses message opcode and parameters
    fn parse<'m>(opcode: Opcode, parameters: &'m [u8]) -> Result<Option<Box<dyn Message + 'm>>, ParseError>
    where
        Self: Sized + 'm;
}

/// Bluetooth Mesh Model Message
pub struct ModelMessage<M> {
    m: M,
}

impl<M> Message for ModelMessage<M>
where
    M: ConcreteMessage,
{
    fn opcode(&self) -> Opcode {
        self.m.opcode()
    }

    fn emit_parameters(&self, xmit: &mut Vec<u8>) {
        let mut v: heapless::Vec<u8, 512> = heapless::Vec::new();
        self.m.emit_parameters(&mut v).unwrap();
        xmit.extend_from_slice(&v[..]);
    }
}

/// Converting Drogue to local types
#[derive(Debug)]
pub struct FromDrogue<M>
where
    M: Debug,
{
    _m: core::marker::PhantomData<M>,
}

impl<M: Debug> FromDrogue<M> {
    /// New model
    pub fn new(_m: M) -> Self {
        Self { _m: core::marker::PhantomData }
    }
}

unsafe impl<M> Sync for FromDrogue<M> where M: Debug {}
unsafe impl<M> Send for FromDrogue<M> where M: Debug {}

impl<M> Model for FromDrogue<M>
where
    M: ConcreteModel + Debug,
{
    fn identifier(&self) -> ModelIdentifier {
        M::IDENTIFIER
    }
    fn supports_subscription(&self) -> bool {
        M::SUPPORTS_SUBSCRIPTION
    }

    fn supports_publication(&self) -> bool {
        M::SUPPORTS_PUBLICATION
    }

    fn parse<'m>(opcode: Opcode, parameters: &'m [u8]) -> Result<Option<Box<dyn Message + 'm>>, ParseError>
    where
        Self: 'm,
    {
        let m = M::parse(&opcode, parameters)?;
        if let Some(m) = m {
            let b: Box<dyn Message + 'm> = Box::new(ModelMessage { m });
            Ok(Some(b))
        } else {
            Ok(None)
        }
    }
}
