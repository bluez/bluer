//! GATT services.

use strum::{Display, EnumString};

pub mod local;
pub mod remote;

pub(crate) const SERVICE_INTERFACE: &str = "org.bluez.GattService1";
pub(crate) const CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
pub(crate) const DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";

define_flags!(CharacteristicFlags, "Bluetooth GATT characteristic flags." => {
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

define_flags!(CharacteristicDescriptorFlags, "Bluetooth GATT characteristic descriptor flags." => {
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

/// Write operation type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString, Display)]
pub enum WriteValueType {
    /// Write without response.
    #[strum(serialize = "command")]
    Command,
    /// Write with response.
    #[strum(serialize = "request")]
    Request,
    /// Reliable write.
    #[strum(serialize = "reliable")]
    Reliable,
}

impl Default for WriteValueType {
    fn default() -> Self {
        Self::Command
    }
}
