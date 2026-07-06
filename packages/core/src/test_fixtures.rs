//! Test fixtures containing precomputed public key, message, and signature.
//! Generated from the Ed25519 seed `[7u8; 32]`.

pub(crate) const TEST_PUBLIC_KEY: [u8; 32] = [
    234, 74, 108, 99, 226, 156, 82, 10, 190, 245, 80, 123, 19, 46, 197, 249, 149, 71, 118, 174,
    190, 190, 123, 146, 66, 30, 234, 105, 20, 70, 210, 44,
];

pub(crate) const TEST_MESSAGE: &[u8] = b"L10K-test-message";

pub(crate) const TEST_SIGNATURE: [u8; 64] = [
    240, 58, 71, 60, 144, 248, 124, 157, 41, 65, 143, 83, 253, 219, 251, 234, 71, 30, 78, 83, 80,
    74, 80, 239, 27, 230, 103, 126, 194, 235, 56, 5, 112, 72, 150, 84, 0, 160, 105, 1, 19, 159, 91,
    193, 241, 241, 179, 254, 151, 20, 151, 59, 249, 44, 113, 11, 27, 44, 62, 49, 63, 32, 199, 7,
];
