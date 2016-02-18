#[derive(Debug)]
pub struct BluetoothDevice {
    address: String,
    name: String,
    class: u32,
    vendor_id: u32,
    product_id: u32,
    product_version: u32,
    uuids: Option<Vec<String>>,
}

impl BluetoothDevice {
    pub fn new(address: String,
           name: String,
           class: u32,
           vendor_id: u32,
           product_id: u32,
           product_version: u32,
           uuids: Option<Vec<String>>)
           -> BluetoothDevice {
        BluetoothDevice {
            address: address,
            name: name,
            class: class,
            vendor_id: vendor_id,
            product_id: product_id,
            product_version: product_version,
            uuids: uuids
        }
    }

    pub fn address(&self) -> String {
      self.address.clone()
    }

    pub fn name(&self) -> String {
      self.name.clone()
    }

    pub fn class(&self) -> u32 {
      self.class
    }

    pub fn vendor_id(&self) -> u32 {
      self.vendor_id
    }

    pub fn product_id(&self) -> u32 {
      self.product_id
    }

    pub fn product_version(&self) -> u32 {
      self.product_version
    }
}