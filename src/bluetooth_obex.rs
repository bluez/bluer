extern crate dbus;
use self::dbus::arg::{Dict, Variant};
use self::dbus::Path as ObjectPath;
use self::dbus::{BusType, Connection, Message, MessageItem, Props};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use bluetooth_device::BluetoothDevice;

const OBEX_BUS: &str = "org.bluez.obex";
const OBEX_PATH: &str = "/org/bluez/obex";
const OBJECT_PUSH_INTERFACE: &str = "org.bluez.obex.ObjectPush1";
const CLIENT_INTERFACE: &str = "org.bluez.obex.Client1";
const TRANSFER_INTERFACE: &str = "org.bluez.obex.Transfer1";

pub enum SessionTarget {
    Ftp,
    Map,
    Opp,
    Pbap,
    Sync_,
}

impl SessionTarget {
    fn as_str(&self) -> &str {
        match self {
            SessionTarget::Ftp => "ftp",
            SessionTarget::Map => "map",
            SessionTarget::Opp => "opp",
            SessionTarget::Pbap => "pbap",
            SessionTarget::Sync_ => "sync",
        }
    }
}

pub enum TransferState {
    Queued,
    Active,
    Complete,
    Suspended,
    Error,
}

impl TransferState {
    fn as_str(&self) -> &str {
        match self {
            TransferState::Queued => "queued",
            TransferState::Active => "active",
            TransferState::Complete => "complete",
            TransferState::Suspended => "suspended",
            TransferState::Error => "error",
        }
    }
}

pub fn open_bus_connection() -> Result<Connection, Box<Error>> {
    let c = Connection::get_private(BusType::Session)?;
    Ok(c)
}

#[derive(Debug)]
pub struct BluetoothOBEXSession {
    connection: Connection,
    object_path: String,
}

impl BluetoothOBEXSession {
    // https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/obex-api.txt#n12
    pub fn new(
        connection: Connection,
        device: &BluetoothDevice,
    ) -> Result<BluetoothOBEXSession, Box<Error>> {
        let device_address: String = device.get_address()?;
        let mut map = HashMap::new();
        map.insert("Target", Variant(SessionTarget::Opp.as_str()));
        let args: Dict<&str, Variant<&str>, _> = Dict::new(map);
        let m = Message::new_method_call(OBEX_BUS, OBEX_PATH, CLIENT_INTERFACE, "CreateSession")?
            .append2(device_address, args);

        let r = connection.send_with_reply_and_block(m, 1000)?;
        let session_path: ObjectPath = r.read1()?;
        let session_str: String = session_path.parse()?;
        let obex_session = BluetoothOBEXSession {
            connection,
            object_path: session_str,
        };
        Ok(obex_session)
    }

    // https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/obex-api.txt#n35
    pub fn remove_session(&self) -> Result<(), Box<Error>> {
        let object_path = ObjectPath::new(self.object_path.as_bytes())?;
        let m = Message::new_method_call(OBEX_BUS, OBEX_PATH, CLIENT_INTERFACE, "RemoveSession")?
            .append1(object_path);
        let _r = self.connection.send_with_reply_and_block(m, 1000)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct BluetoothOBEXTransfer<'a> {
    session: &'a BluetoothOBEXSession,
    object_path: String,
    name: String,
}

impl<'a> BluetoothOBEXTransfer<'a> {
    // https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/obex-api.txt#n169
    pub fn send_file(
        session: &'a BluetoothOBEXSession,
        file_path: &str,
    ) -> Result<BluetoothOBEXTransfer<'a>, Box<Error>> {
        let session_path: String = session.object_path.clone();
        let m =
            Message::new_method_call(OBEX_BUS, session_path, OBJECT_PUSH_INTERFACE, "SendFile")?
                .append1(file_path);
        let r = session.connection.send_with_reply_and_block(m, 1000)?;
        let transfer_path: ObjectPath = r.read1()?;
        let transfer_str: String = transfer_path.parse()?;

        let file_name: String = match Path::new(file_path).file_name() {
            Some(value) => value.to_string_lossy().to_string(),
            None => file_path.to_string(),
        };

        let obex_transfer = BluetoothOBEXTransfer {
            session,
            object_path: transfer_str,
            name: file_name,
        };
        Ok(obex_transfer)
    }

    // https://git.kernel.org/pub/scm/bluetooth/bluez.git/tree/doc/obex-api.txt#n115
    pub fn status(&self) -> Result<String, Box<Error>> {
        let transfer_path = self.object_path.clone();
        let p = Props::new(
            &self.session.connection,
            OBEX_BUS,
            transfer_path,
            TRANSFER_INTERFACE,
            1000,
        );
        let status: MessageItem = p.get("Status")?;
        match status.inner::<&str>() {
            Ok(value) => Ok(value.to_string()),
            Err(_) => Err("Failed to get status.".into()),
        }
    }

    pub fn wait_until_transfer_completed(&self) -> Result<(), Box<Error>> {
        sleep(Duration::from_millis(500));
        let mut transfer_status: String = self.status()?;

        while transfer_status != TransferState::Complete.as_str() {
            sleep(Duration::from_millis(500));
            transfer_status = match self.status() {
                Ok(value) => {
                    if value == TransferState::Error.as_str() {
                        break;
                    } else {
                        value
                    }
                }
                Err(_) => break,
            }
        }
        Ok(())
    }
}
