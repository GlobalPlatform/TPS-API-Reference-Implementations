/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
// Default configuration
#![no_std]

/// # ROT13 Service
///
/// > Note: It hardly needs saying that ROT13 provides no security whatsoever - it is the canonical
/// > example of a weak cipher. This code is purely for educational purposes.
///
/// This is a minimal example of a TPS Service. It is not useful in any reasonable production sense
/// but does show how a TPS Service can be constructed, including error handling, message handling
/// and interface to a Connector.
///
/// The ROT13 service performs "encryption" and "decryption" of messages consisting of only upper
/// and lower case characters from the 26 characters of the English language alphabet. If the
/// input message contains any other character, the message is rejected as erroneous.
///
/// > Note: This code has been written for simplicity and clarity more than for optimized
/// > performance and/or code size. There are definitely smaller/faster ways to do things.
///
/// # Messages Supported
///
/// - `TPS_GetFeatures_Req`: #6.1
/// - `TPS_GetFeatures_Rsp`: #6.1
/// - `GPP_ROT13_Encrypt_Req`: #6.10
/// - `GPP_ROT13_Encrypt_Rsp`: #6.10
/// - `GPP_ROT13_Decrypt_Req`: #6.11
/// - `GPP_ROT13_Decrypt_Rsp`: #6.11
///
///  The underlying implementations of encryption and decryption in ROT13 are identical, as ROT13
///  is reversible. It thus really doesn't matter which is called as they do the same thing.
///
/// ## Service Discovery
///
/// Like all TPS Services, it is required to respond to the standard connector message
/// `TPS_GetFeatures_Req`, returning `TPS_GetFeatures_Rsp`.
///
/// For this service, `TPS_GetFeatures_Rsp` returns the following:
///
/// - `svc_name`: "GPP ROT13" (tstr)
/// - `login_method`: [0]
///
/// - `$$svc_features` //= (128 => [0, 1])   // "encrypt" and "decrypt"
///
/// ## Encrypt
///
/// `GPP_ROT13_Encrypt_Req` is encoded in CDDL as follows:
///
/// ```cddl
/// GPP_ROT13_Encrypt_Req = #6.10 ({
///   1 => tstr
/// })
/// ```
///
/// The service responds with `GPP_ROT13_Encrypt_Rsp`, which is encoded as follows:
///
///  ```cddl
/// GPP_ROT13_Encrypt_Rsp = #6.10 ({
///   (1 => tstr / 2 => uint)
/// })
/// ```
///
/// If map item 1 is present, "encryption" was successful and the `tstr` contains the "encrypted"
/// payload. If map item 2 is present, "encryption" failed and the `uint` value provides a helpful
/// error code as follows:
///
/// - 1: space character detected
/// - 2: numeric character detected
/// - 3: some other symbol detected
/// - 4: input too large (`MAX_STRING_SIZE` bytes or more)
///
/// ## Decrypt
///
/// `GPP_ROT13_Decrypt_Req` is encoded in CDDL as follows:
///
/// ```cddl
/// GPP_ROT13_Decrypt_Req = #6.11 ({
///   1 => tstr
/// })
/// ```
///
/// The service responds with `GPP_ROT13_Encrypt_Rsp`, which is encoded as follows:
///
///  ```cddl
/// GPP_ROT13_Decrypt_Rsp = #6.11 ({
///   (1 => tstr / 2 => uint)
/// })
/// ```
///
/// If map item 1 is present, "decryption" was successful and the `tstr` contains the "decrypted"
/// payload. If map item 2 is present, "decryption" failed and the `uint` value provides a helpful
/// error code as follows:
///
/// - 1: space character detected
/// - 2: numeric character detected
/// - 3: some other symbol detected
/// - 4: input too large (`MAX_STRING_SIZE` bytes or more)
// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "std", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

extern crate tps_minicbor;
extern crate tps_client_common;

use std::mem::size_of;

use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_structs::ServiceVersion;
use tps_minicbor::{DecodeError, Decoder, EncodeError, EncoderStd, Item, TaggedItem};

/***************************************************************************************************
 * Constants
 **************************************************************************************************/

/// Standard message tag for all TPS Services: TPS_GetFeatures_Req
const TPS_GET_FEATURES_REQ: u32 = 1;
/// Standard message tag for all TPS Services: TPS_GetFeatures_Rsp
const TPS_GET_FEATURES_RSP: u32 = 1;

/// Standard message key for TPS_GetFeatures_Req: `svc_name`
const TPS_GET_FEATURES_SVC_NAME_KEY: u32 = 1;
/// Standard message key for TPS_GetFeatures_Req: `login_method`
const TPS_GET_FEATURES_LOGIN_METHOD_KEY: u32 = 2;
/// Standard message key for TPS_GetFeatures_Req: `profile_name`
#[allow(dead_code)]
const TPS_GET_FEATURES_PROFILE_NAME_KEY: u32 = 3; // not used

/// ROT 13 Service message tag: GPP_Rot13_Encrypt_Req
pub const GPP_ROT13_ENCRYPT_REQ: u32 = 10;
/// ROT 13 Service message tag: GPP_Rot13_Encrypt_Rsp
pub const GPP_ROT13_ENCRYPT_RSP: u32 = 10;
/// ROT 13 Service message tag: GPP_Rot13_Decrypt_Req
pub const GPP_ROT13_DECRYPT_REQ: u32 = 11;
/// ROT 13 Service message tag: GPP_Rot13_Decrypt_Rsp
pub const GPP_ROT13_DECRYPT_RSP: u32 = 11;

/// ROT13 service features: encrypt service
pub const GPP_ROT13_ENCRYPT_SERVICE: u32 = 0;
/// ROT13 service features: decrypt service
pub const GPP_ROT13_DECRYPT_SERVICE: u32 = 1;

/// UUID uniquely identifying the ROT13 service
pub const GPP_ROT13_SERVICE_NAME: [u8; 16] = [
    0x87, 0xba, 0xe7, 0x13, 0xb0, 0x8f, 0x5e, 0x28, 0xb9, 0xee, 0x4a, 0xa6, 0xe2, 0x02, 0x44, 0x0e,
];

/// `svc_name` value reported in `TPS_GetFeatures_Rsp`.
pub const GPP_ROT13_SVC_NAME: &str = "GPP ROT13";

/// UUID uniquely identifying this secure component type.
///
/// We do not use one of the standard SC types in this implementation as we are not a real secure
/// component. This UUID was constructed from the TPS namespace and name "Insecure Test SC".
pub const GPP_TEST_SC_TYPE: [u8; 16] = [
    0x0a, 0x28, 0x8f, 0x23, 0xb7, 0x36, 0x58, 0x26, 0xa8, 0x1a, 0x9f, 0xe6, 0x6e, 0x33, 0x16, 0x61,
];

/// The version of the service
pub const GPP_ROT13_SERVICE_VERSION: ServiceVersion = ServiceVersion {
    major_version: 0,
    minor_version: 0,
    patch_version: 16,
};

/// Standard message key for TPS_GetFeatures_Req: `login_method`
pub const GPP_SVC_FEATURES_KEY: u32 = 0x80;
pub const GPP_ROT13_CIPHERTEXT_KEY: u32 = 1;
pub const GPP_ROT13_PLAINTEXT_KEY: u32 = 1;
pub const GPP_ROT13_ERROR_KEY: u32 = 2;

pub const GPP_ROT13_ERROR_SPACE: u32 = 1;
pub const GPP_ROT13_ERROR_NUMERIC: u32 = 2;
pub const GPP_ROT13_ERROR_OTHER: u32 = 3;
pub const GPP_ROT13_ERROR_TOO_LARGE: u32 = 4;

pub const MAX_STRING_SIZE: usize = 256 * size_of::<char>();

#[derive(Copy, Clone, Debug, PartialEq)]
enum Rot13Operation {
    Encode,
    Decode,
}

/// Error returned by [`message_handler`], combining CBOR decode and encode errors.
#[derive(Debug)]
pub enum Rot13Error {
    Decode(DecodeError),
    Encode(EncodeError),
}

impl From<DecodeError> for Rot13Error {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<EncodeError> for Rot13Error {
    fn from(e: EncodeError) -> Self {
        Self::Encode(e)
    }
}

/***************************************************************************************************
 * Rust Entry Point
 **************************************************************************************************/
/// The `message_handler` function initiates all of the message handling for this (very simplified)
/// service definition.
///
/// It receives borrowed references to a `CBORDecoder` and a `CBOREncoder` which are used to decode
/// the incoming message and encode the outgoing message, respectively. One consequence is that at
/// the level of this service, separate buffers are used for input and output.
///
/// Messages are dispatched to the appropriate handler function based on the message tag. This
/// service supports tags 1 (TPS_GetFeatures_Req/Rsp), 10 (GPP_ROT13_Encrypt_Req/Rsp) and 11
/// (GPP_ROT13_Decrypt_Req/Rsp)
///
/// In this function we are generally returning `Err(Rot13Error)`, which will be converted into
/// a `u32` before it is passed back to the connector.
pub fn message_handler<'b>(
    in_msg_buf: &'b [u8],
    out_msg_buf: &'b mut [u8],
) -> Result<usize, Rot13Error> {
    // The tag contains the message ID
    let tagged = match Decoder::new(in_msg_buf).decode()? {
        Item::Tag(tagged) => tagged,
        _ => return Err(DecodeError::UnexpectedItem.into()),
    };
    let msg_id = u32::try_from(tagged.tag).map_err(|_| DecodeError::OutOfRange)?;
    // Sized to comfortably exceed the largest single response this service can produce: a
    // tag + one-entry map wrapping a `tstr` of at most `MAX_STRING_SIZE` bytes, plus a small
    // margin for CBOR header overhead.
    let mut overflow_scratch: [u8; MAX_STRING_SIZE + 32] = [0; MAX_STRING_SIZE + 32];
    let mut encoder = EncoderStd::new_with_overflow(out_msg_buf, &mut overflow_scratch);
    match msg_id {
        // GET_FEATURES_REQ (tag 1) - Don't care about contents
        TPS_GET_FEATURES_REQ => handle_get_features(&mut encoder),
        // ROT13_ENCRYPT_REQ (tag 10) - we should have { 1: tstr }
        GPP_ROT13_ENCRYPT_REQ => {
            let text_body = rot13_req_text(&tagged)?;
            handle_encrypt_req(text_body, &mut encoder)
        }
        // ROT13_DECRYPT_REQ (tag 11) - we should have { 1: tstr }
        GPP_ROT13_DECRYPT_REQ => {
            let text_body = rot13_req_text(&tagged)?;
            handle_decrypt_req(text_body, &mut encoder)
        }
        _ => return Err(DecodeError::UnexpectedItem.into()),
    }?;
    Ok(encoder.encoded().len())
}

/// Extract the `tstr` at map key 1 from a request body of the form `{ 1 => tstr }`.
fn rot13_req_text<'b>(tagged: &TaggedItem<'b>) -> Result<&'b str, Rot13Error> {
    let map = match tagged.item()? {
        Item::Map(map) => map,
        _ => return Err(DecodeError::UnexpectedItem.into()),
    };
    for pair in map.iter() {
        let (key, value) = pair?;
        if matches!(key, Item::Unsigned(1)) {
            return Ok(<&str>::try_from(value)?);
        }
    }
    Err(DecodeError::UnexpectedItem.into())
}

/// Handler for the `TPS_GetFeatures_Req/Rsp` message pair.
///
/// In this case, as the `TPS_GetFeatures_Req` message is so simple, it is handled entirely by the
/// caller. For many messages, this will not be the case. This is the reason for having only an
/// `EncoderStd` parameter to the function.
fn handle_get_features(encoder: &mut EncoderStd) -> Result<(), Rot13Error> {
    let mut msg = encoder.tag(TPS_GET_FEATURES_RSP.into())?.map()?;
    // 1 => svc_name (tstr)
    msg.insert_key_value(TPS_GET_FEATURES_SVC_NAME_KEY, GPP_ROT13_SVC_NAME)?;
    // 2 => [+ login_method]
    let mut login_method = msg.key(TPS_GET_FEATURES_LOGIN_METHOD_KEY)?.array()?;
    login_method.insert(LOGIN_PUBLIC)?;
    login_method.end()?;
    // 0x80 => [0, 1]
    let mut features = msg.key(GPP_SVC_FEATURES_KEY)?.array()?;
    features.insert(GPP_ROT13_ENCRYPT_SERVICE)?;
    features.insert(GPP_ROT13_DECRYPT_SERVICE)?;
    features.end()?;
    msg.end()?;
    Ok(())
}

/// Handler for the `GPP_ROT13_Encrypt_Req/Rsp` message pair.
fn handle_encrypt_req(plaintext: &str, encoder: &mut EncoderStd) -> Result<(), Rot13Error> {
    rot13_req_helper(Rot13Operation::Encode, plaintext, encoder)
}

/// Handler for the `GPP_ROT13_Decrypt_Req/Rsp` message pair.
fn handle_decrypt_req(plaintext: &str, encoder: &mut EncoderStd) -> Result<(), Rot13Error> {
    rot13_req_helper(Rot13Operation::Decode, plaintext, encoder)
}

fn rot13_req_helper(
    op: Rot13Operation,
    plaintext: &str,
    encoder: &mut EncoderStd,
) -> Result<(), Rot13Error> {
    let (msg_id, text_key) = if op == Rot13Operation::Encode {
        (GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_CIPHERTEXT_KEY)
    } else {
        (GPP_ROT13_DECRYPT_RSP, GPP_ROT13_PLAINTEXT_KEY)
    };

    // Want this to work as no_std, so we cannot use strings. There are some contortions here
    // with a stack allocated [u8] which we later convert to &str using core::str::from_utf8()
    let mut ciphertext_buf: [u8; MAX_STRING_SIZE] = [0; MAX_STRING_SIZE];

    match rot13(op, plaintext, ciphertext_buf.as_mut_slice()) {
        Ok(ciphertext_len) => {
            let ciphertext = core::str::from_utf8(&ciphertext_buf.as_slice()[0..ciphertext_len])
                .map_err(|_| DecodeError::InvalidUtf8)?;
            let mut msg = encoder.tag(msg_id.into())?.map()?;
            msg.insert_key_value(text_key, ciphertext)?;
            msg.end()?;
            Ok(())
        }
        Err(error_code) => {
            let mut msg = encoder.tag(msg_id.into())?.map()?;
            msg.insert_key_value(GPP_ROT13_ERROR_KEY, error_code)?;
            msg.end()?;
            Ok(())
        }
    }
}

fn rot13(operation: Rot13Operation, input: &str, output: &mut [u8]) -> Result<usize, u32> {
    // In this function we only alter values in the ASCII 'A'-'Z', 'a'-'z' range, which means that
    // input_bytes is always a valid unicode string
    let a_lower = b'a';
    let a_upper = b'A';

    if input.is_empty() {
        Err(GPP_ROT13_ERROR_OTHER)
    } else if input.len() >= MAX_STRING_SIZE {
        Err(GPP_ROT13_ERROR_TOO_LARGE)
    } else {
        let mut idx = 0;
        for character in input.chars() {
            if character.is_ascii_uppercase() || character.is_ascii_lowercase() {
                let char_val: u8 = char::try_into(character).unwrap(); // Infallible for upper and lower case ASCII
                let char_pos = char_val
                    - if character.is_ascii_uppercase() {
                        a_upper
                    } else {
                        a_lower
                    };
                let shifted_char_pos = match operation {
                    Rot13Operation::Encode => (char_pos + 13) % 26,
                    Rot13Operation::Decode => {
                        if char_pos < 13 {
                            char_pos + 13
                        } else {
                            char_pos - 13
                        }
                    }
                };
                let shifted_char = u8::into(
                    shifted_char_pos
                        + if character.is_ascii_uppercase() {
                            a_upper
                        } else {
                            a_lower
                        },
                );
                output[idx] = shifted_char;
                idx += 1;
            } else if character.is_ascii_whitespace() {
                return Err(GPP_ROT13_ERROR_SPACE);
            } else if character.is_ascii_digit() {
                return Err(GPP_ROT13_ERROR_NUMERIC);
            } else {
                return Err(GPP_ROT13_ERROR_OTHER);
            }
        }
        Ok(idx)
    }
}
