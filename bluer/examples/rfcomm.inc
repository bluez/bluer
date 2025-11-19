const CHANNEL: u8 = 1;

const HELLO_MSG: &[u8] = b"Hello from rfcomm_server!";

const PROFILE_UUID: bluer::Uuid = bluer::Uuid::from_u128(0x12345678_1234_5678_1234_567812345678);
