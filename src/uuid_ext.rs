use uuid::Uuid;

/// UUID extension trait to convert to and from Bluetooth short UUIDs.
pub trait UuidExt {
    /// 32-bit short form of Bluetooth UUID.
    fn as_u32(&self) -> Option<u32>;
    /// 16-bit short form of Bluetooth UUID.
    fn as_u16(&self) -> Option<u16>;
    /// Long form of 32-bit short form Bluetooth UUID.
    fn from_u32(v: u32) -> Uuid;
    /// Long form of 16-bit short form Bluetooth UUID.
    fn from_u16(v: u16) -> Uuid;
}

const BASE_UUID: u128 = 0x00000000_0000_1000_8000_00805f9b34fb;
const BASE_MASK_32: u128 = 0x00000000_ffff_ffff_ffff_ffffffffffff;
const BASE_MASK_16: u128 = 0xffff0000_ffff_ffff_ffff_ffffffffffff;

impl UuidExt for Uuid {
    fn as_u32(&self) -> Option<u32> {
        let value = self.as_u128();
        if value & BASE_MASK_32 == BASE_UUID {
            Some((value >> 96) as u32)
        } else {
            None
        }
    }

    fn as_u16(&self) -> Option<u16> {
        let value = self.as_u128();
        if value & BASE_MASK_16 == BASE_UUID {
            Some((value >> 96) as u16)
        } else {
            None
        }
    }

    fn from_u32(v: u32) -> Uuid {
        Uuid::from_u128(BASE_UUID | ((v as u128) << 96))
    }

    fn from_u16(v: u16) -> Uuid {
        Uuid::from_u128(BASE_UUID | ((v as u128) << 96))
    }
}
