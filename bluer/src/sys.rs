//! System native types and constants.

use libc::{c_ushort, sa_family_t};

#[repr(C)]
#[derive(Clone)]
pub struct bt_security {
    pub level: u8,
    pub key_size: u8,
}

pub const BT_SECURITY: i32 = 4;
pub const BT_SECURITY_SDP: i32 = 0;
pub const BT_SECURITY_LOW: i32 = 1;
pub const BT_SECURITY_MEDIUM: i32 = 2;
pub const BT_SECURITY_HIGH: i32 = 3;
pub const BT_SECURITY_FIPS: i32 = 4;

#[repr(C)]
#[derive(Clone)]
pub struct bt_power {
    pub force_active: u8,
}

pub const BT_POWER: i32 = 9;
pub const BT_POWER_FORCE_ACTIVE_OFF: i32 = 0;
pub const BT_POWER_FORCE_ACTIVE_ON: i32 = 1;

pub const BT_SNDMTU: i32 = 12;
pub const BT_RCVMTU: i32 = 13;

pub const BT_MODE: i32 = 15;

pub const BTPROTO_L2CAP: i32 = 0;

#[repr(packed)]
#[repr(C)]
#[derive(Clone)]
pub struct bdaddr_t {
    pub b: [u8; 6],
}

pub const BDADDR_BREDR: u8 = 0x00;
pub const BDADDR_LE_PUBLIC: u8 = 0x01;
pub const BDADDR_LE_RANDOM: u8 = 0x02;

#[repr(C)]
#[derive(Clone)]
pub struct sockaddr_l2 {
    pub l2_family: sa_family_t,
    pub l2_psm: c_ushort,
    pub l2_bdaddr: bdaddr_t,
    pub l2_cid: c_ushort,
    pub l2_bdaddr_type: u8,
}
