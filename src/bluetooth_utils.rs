use crate::{Error, Result, SERVICE_NAME, TIMEOUT};
use dbus::nonblock::Proxy;
use dbus::{
    arg::{Append, AppendAll, Arg, Get, PropMap, ReadAll},
    nonblock::{
        stdintf::org_freedesktop_dbus::{ObjectManager, Properties},
        SyncConnection,
    },
    Path,
};
use hex::FromHex;
use std::{collections::HashMap, str::FromStr, time::Duration};

static ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
static DEVICE_INTERFACE: &str = "org.bluez.Device1";
static SERVICE_INTERFACE: &str = "org.bluez.GattService1";
static CHARACTERISTIC_INTERFACE: &str = "org.bluez.GattCharacteristic1";
static DESCRIPTOR_INTERFACE: &str = "org.bluez.GattDescriptor1";
static LEADVERTISING_DATA_INTERFACE: &str = "org.bluez.LEAdvertisement1";
static LEADVERTISING_MANAGER_INTERFACE: &str = "org.bluez.LEAdvertisingManager1";


async fn get_managed_objects(c: &SyncConnection) -> Result<HashMap<Path<'static>, HashMap<String, PropMap>>> {
    let p = Proxy::new(SERVICE_NAME, "/", TIMEOUT, c);
    Ok(p.get_managed_objects().await?)
}

pub async fn get_adapters(c: &SyncConnection) -> Result<Vec<String>> {
    let mut adapters = Vec::new();
    let objects = get_managed_objects(&c).await?;
    for (path, interfaces) in objects {
        for (name, _) in interfaces {
            if name == ADAPTER_INTERFACE {
                adapters.push(path.to_string());
            }
        }
    }
    Ok(adapters)
}

// pub fn get_ad_man() -> Result<Vec<String>> {
//     let mut managers = Vec::new();
//     let c = Connection::get_private(BusType::System)?;
//     let objects = get_managed_objects(&c)?;
//     let z: &[(MessageItem, MessageItem)] =
//         ok_or_str!(or_else_str!(objects.get(0), "get_ad_mancouldn't get managed objects")?.inner())?;
//     for (path, interfaces) in z {
//         let x: &[(MessageItem, MessageItem)] = ok_or_str!(interfaces.inner())?;
//         for (i, _) in x {
//             let name: &str = ok_or_str!(i.inner())?;
//             if name == LEADVERTISING_MANAGER_INTERFACE {
//                 let p: &str = ok_or_str!(path.inner())?;
//                 managers.push(String::from(p));
//             }
//         }
//     }
//     Ok(managers)
// }

pub async fn list_devices(c: &SyncConnection, adapter_path: &str) -> Result<Vec<String>> {
    list_item(c, DEVICE_INTERFACE, adapter_path, "Adapter").await
}

pub async fn list_services(c: &SyncConnection, device_path: &str) -> Result<Vec<String>> {
    list_item(c, SERVICE_INTERFACE, device_path, "Device").await
}

pub async fn list_characteristics(c: &SyncConnection, device_path: &str) -> Result<Vec<String>> {
    list_item(c, CHARACTERISTIC_INTERFACE, device_path, "Service").await
}

pub async fn list_descriptors(c: &SyncConnection, device_path: &str) -> Result<Vec<String>> {
    list_item(c, DESCRIPTOR_INTERFACE, device_path, "Characteristic").await
}

pub async fn list_addata_1(c: &SyncConnection, adapter_path: &str) -> Result<Vec<String>> {
    list_item(c, LEADVERTISING_DATA_INTERFACE, adapter_path, "Advertisement").await
}

pub async fn list_addata_2(c: &SyncConnection, device_path: &str) -> Result<Vec<String>> {
    list_item(c, LEADVERTISING_DATA_INTERFACE, device_path, "Advertisement").await
}

async fn list_item(
    c: &SyncConnection, item_interface: &str, item_path: &str, item_property: &str,
) -> Result<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let objects = get_managed_objects(&c).await?;
    for (path, interfaces) in objects {
        for (name, _) in interfaces {
            if name == item_interface {
                let prop_path: String = get_property(c, item_interface, &path.to_string(), item_property).await?;
                if prop_path == item_path {
                    v.push(path.to_string());
                }
            }
        }
    }
    Ok(v)
}

pub async fn get_property<R>(c: &SyncConnection, interface: &str, object_path: &str, prop: &str) -> Result<R>
where
    R: for<'b> Get<'b> + 'static,
{
    let p = Proxy::new(SERVICE_NAME, object_path, TIMEOUT, c);
    Ok(p.get(interface, prop).await?)
}

pub async fn set_property<T>(
    c: &SyncConnection, interface: &str, object_path: &str, prop: &str, value: T,
) -> Result<()>
where
    T: Arg + Append,
{
    let p = Proxy::new(SERVICE_NAME, object_path, TIMEOUT, c);
    p.set(interface, prop, value).await?;
    Ok(())
}

pub async fn call_method<A, R>(
    c: &SyncConnection, interface: &str, object_path: &str, method: &str, param: A, timeout: Duration,
) -> Result<R>
where
    A: AppendAll,
    R: ReadAll + 'static,
{
    let p = Proxy::new(SERVICE_NAME, object_path, timeout, c);
    Ok(p.method_call(interface, method, param).await?)
}

