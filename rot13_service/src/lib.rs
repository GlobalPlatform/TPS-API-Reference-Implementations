/***************************************************************************************************
 * Copyright (c) 2022, Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all copies or
 * substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
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
/// - `svc_name`: h'87bae713b08f5e28b9ee4aa6e202440e'
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
// Pull in std if we are testing or if it is defined as feature (because we run tests on a
// platform supporting I/O and full feature set.
#[cfg(any(feature = "std", test))]
extern crate std;

// If we are really building no_std, pull in core as well. It is aliased as std so that "use"
// statements are always the same
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

extern crate rs_minicbor;
extern crate tps_client_common;

use std::mem::size_of;

use rs_minicbor::decoder::{is_map, is_tag, CBORDecoder, SequenceBuffer};
use rs_minicbor::encoder::CBORBuilder;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::{array, map, tag, CBOR};
use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_structs::ServiceVersion;

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
/// In this function we are generally returning `Err(CBORError)`, which will be converted into
/// a `u32` before it is passed back to the connector.
pub fn message_handler<'b>(
    in_msg_buf: &'b [u8],
    out_msg_buf: &'b mut [u8],
) -> Result<(), CBORError> {
    let mut decoder = CBORDecoder::new(SequenceBuffer::new(in_msg_buf));
    // The tag contains the message ID
    decoder.decode_with(is_tag(), |cbor| {
        let mut msg_id: u64 = 0;
        let mut msg_body = CBORDecoder::from_tag(cbor, &mut msg_id)?;
        let mut encoder = CBORBuilder::new(out_msg_buf);
        match msg_id as u32 {
            // GET_FEATURES_REQ (tag 1) - Don't care about contents
            TPS_GET_FEATURES_REQ => handle_get_features(&mut encoder),
            // ROT13_ENCRYPT_REQ (tag 10) - we should have { 1: tstr }
            GPP_ROT13_ENCRYPT_REQ => {
                let _ = msg_body.decode_with(is_map(), |cbor| {
                    let mut encoder = CBORBuilder::new(out_msg_buf);
                    if let CBOR::Map(mb) = cbor {
                        // Get the CBOR item at key == 1, which should be a tstr
                        if let Some(text_body) = mb.get_int(1) {
                            handle_encrypt_req(&text_body, &mut encoder)
                        } else {
                            Err(CBORError::IncompatibleType)
                        }
                    } else {
                        Err(CBORError::IncompatibleType)
                    }
                })?;
                Ok(())
            }
            // ROT13_ENCRYPT_REQ (tag 11) - we should have { 1: tstr }
            GPP_ROT13_DECRYPT_REQ => {
                let _ = msg_body.decode_with(is_map(), |cbor| {
                    let mut encoder = CBORBuilder::new(out_msg_buf);
                    if let CBOR::Map(mb) = cbor {
                        // Get the CBOR item at key == 1, which should be a tstr
                        if let Some(text_body) = mb.get_int(1) {
                            handle_decrypt_req(&text_body, &mut encoder)
                        } else {
                            Err(CBORError::IncompatibleType)
                        }
                    } else {
                        Err(CBORError::IncompatibleType)
                    }
                })?;
                Ok(())
            }
            _ => Err(CBORError::IncompatibleType),
        }
    })?;
    Ok(())
}

/// Handler for the `TPS_GetFeatures_Req/Rsp` message pair.
///
/// In this case, as the `TPS_GetFeatures_Req` message is so simple, it is handled entirely by the
/// caller. For many messages, this will not be the case. This is the reason for having only a
/// `CBOREncoder` parameter to the function.
fn handle_get_features(encoder: &mut CBORBuilder) -> Result<(), CBORError> {
    match encoder
        // Tag: Message ID
        .insert(&tag(TPS_GET_FEATURES_RSP.into(), |buf| {
            buf.insert(&map(|buf| {
                buf
                    // 1 => svc_name (bstr .size 16)
                    .insert_key_value(
                        &TPS_GET_FEATURES_SVC_NAME_KEY,
                        &GPP_ROT13_SERVICE_NAME.as_slice(),
                    )?
                    // 2 => [+ login_method]
                    .insert_key_value(
                        &TPS_GET_FEATURES_LOGIN_METHOD_KEY,
                        &array(|buf| buf.insert(&LOGIN_PUBLIC)),
                    )?
                    // 0x80 => [0, 1]
                    .insert_key_value(
                        &GPP_SVC_FEATURES_KEY,
                        &array(|buf| {
                            buf.insert(&GPP_ROT13_ENCRYPT_SERVICE)?
                                .insert(&GPP_ROT13_DECRYPT_SERVICE)
                        }),
                    )
            }))
        })) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Handler for the `TPS_GetFeatures_Req/Rsp` message pair.
///
/// In this case, as the `TPS_GetFeatures_Req` message is so simple, it is handled entirely by the
/// caller. For many messages, this will not be the case. This is the reason for having only a
/// `CBOREncoder` parameter to the function.
fn handle_encrypt_req<'b>(
    decoder: &'b CBOR<'b>,
    encoder: &'b mut CBORBuilder<'b>,
) -> Result<(), CBORError> {
    rot13_req_helper(Rot13Operation::Encode, decoder, encoder)
}

fn handle_decrypt_req<'b>(
    decoder: &'b CBOR<'b>,
    encoder: &'b mut CBORBuilder<'b>,
) -> Result<(), CBORError> {
    rot13_req_helper(Rot13Operation::Decode, decoder, encoder)
}

fn rot13_req_helper<'b>(
    op: Rot13Operation,
    decoder: &'b CBOR<'b>,
    encoder: &'b mut CBORBuilder<'b>,
) -> Result<(), CBORError> {
    let may_plaintext = decoder.try_into_str();

    let (msg_id, text_key) = if op == Rot13Operation::Encode {
        (GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_CIPHERTEXT_KEY)
    } else {
        (GPP_ROT13_DECRYPT_RSP, GPP_ROT13_PLAINTEXT_KEY)
    };

    if let Ok(plaintext) = may_plaintext {
        // Want this to work as no_std, so we cannot use strings. There are some contortions here
        // with a stack allocated [u8] which we later convert to &str using core::str::from_utf8()
        let mut ciphertext_buf: [u8; MAX_STRING_SIZE] = [0; MAX_STRING_SIZE];

        match rot13(op, plaintext, &mut ciphertext_buf.as_mut_slice()) {
            Ok(ciphertext_len) => {
                match core::str::from_utf8(&ciphertext_buf.as_slice()[0..ciphertext_len]) {
                    Ok(ciphertext) => {
                        encoder.insert(&tag(msg_id as u64, |buf| {
                            buf.insert(&map(|buf| buf.insert_key_value(&text_key, &ciphertext)))
                        }))?;
                        Ok(())
                    }
                    Err(_) => Err(CBORError::UTF8Error),
                }
            }
            Err(e) => {
                let error_code = CBOR::UInt(e as u64);
                encoder.insert(&tag(msg_id as u64, |buf| {
                    buf.insert(&map(|buf| {
                        buf.insert_key_value(&GPP_ROT13_ERROR_KEY, &error_code)
                    }))
                }))?;
                Ok(())
            }
        }
    } else {
        Err(CBORError::MalformedEncoding)
    }
}

fn rot13<'b>(operation: Rot13Operation, input: &str, output: &mut [u8]) -> Result<usize, u32> {
    // In this function we only alter values in the ASCII 'A'-'Z', 'a'-'z' range, which means that
    // input_bytes is always a valid unicode string
    let a_lower = u8::from(b'a');
    let a_upper = u8::from(b'A');

    if input.len() == 0 {
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
