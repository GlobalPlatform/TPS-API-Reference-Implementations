/***************************************************************************************************
 * Copyright (c) 2022 Jeremy O'Donoghue. All rights reserved.
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

extern crate rot13_service;

use rot13_service::{
    message_handler, GPP_ROT13_CIPHERTEXT_KEY, GPP_ROT13_DECRYPT_REQ, GPP_ROT13_DECRYPT_RSP,
    GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_ERROR_NUMERIC, GPP_ROT13_ERROR_OTHER,
    GPP_ROT13_ERROR_SPACE, GPP_ROT13_PLAINTEXT_KEY,
};
use rs_minicbor::decoder::{is_map, is_tag_with_value, SequenceBuffer};
use rs_minicbor::encoder::CBORBuilder;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::{map, tag, CBOR};

#[test]
fn test_encode_lowercase_success() -> Result<(), CBORError> {
    let plaintext = "thequickbrownfoxjumpsoverthelazydog";
    let ciphertext = "gurdhvpxoebjasbkwhzcfbiregurynmlqbt";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_ENCRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_PLAINTEXT_KEY, &plaintext)
                }))
            }))?
            .encoded()?;
        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_ENCRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(1) {
                    Some(cbor) => match cbor {
                        CBOR::Tstr(receive_text) => assert_eq!(receive_text, ciphertext),
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_encode_uppercase_success() -> Result<(), CBORError> {
    let plaintext = "THEQUICKBROWNFOXJUMPSOVERTHELAZYDOG";
    let ciphertext = "GURDHVPXOEBJASBKWHZCFBIREGURYNMLQBT";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_ENCRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_PLAINTEXT_KEY, &plaintext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_ENCRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(1) {
                    Some(cbor) => match cbor {
                        CBOR::Tstr(receive_text) => assert_eq!(receive_text, ciphertext),
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_encode_digit_failure() -> Result<(), CBORError> {
    let plaintext = "thequickbrownfox9umpsoverthelazydog";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_ENCRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_PLAINTEXT_KEY, &plaintext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_ENCRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_NUMERIC as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_decode_digit_failure() -> Result<(), CBORError> {
    let ciphertext = "GURDHVPXOEBJ4SBKWHZCFBIREGURYNMLQBT";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_DECRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_CIPHERTEXT_KEY, &ciphertext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_DECRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_NUMERIC as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_encode_space_failure() -> Result<(), CBORError> {
    let plaintext = "the quick brown fox jumps over the lazy dog";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_ENCRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_PLAINTEXT_KEY, &plaintext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_ENCRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_SPACE as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_decode_space_failure() -> Result<(), CBORError> {
    let ciphertext = "GUR DHVPX OEBJA SBK WHZCF BIRE GUR YNML QBT";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_DECRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_CIPHERTEXT_KEY, &ciphertext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_DECRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_SPACE as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_encode_other_failure() -> Result<(), CBORError> {
    let plaintext = "";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_ENCRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_PLAINTEXT_KEY, &plaintext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_ENCRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_OTHER as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}

#[test]
fn test_decode_other_failure() -> Result<(), CBORError> {
    let ciphertext = "";
    let mut send_buf = [0u8; 100];
    let mut receive_buf = [0u8; 100];

    // Mutable borrows start here
    {
        let mut encoder = CBORBuilder::new(&mut send_buf);
        let encode_buf = encoder
            .insert(&tag(GPP_ROT13_DECRYPT_REQ as u64, |buf| {
                buf.insert(&map(|buf| {
                    buf.insert_key_value(&GPP_ROT13_CIPHERTEXT_KEY, &ciphertext)
                }))
            }))?
            .encoded()?;

        message_handler(encode_buf, &mut receive_buf)?;
    }
    {
        let decode_iter = SequenceBuffer::new(&mut receive_buf).into_iter();
        if let (_, CBOR::Tag(tb)) = is_tag_with_value(GPP_ROT13_DECRYPT_RSP as u64)(decode_iter)? {
            if let (_, CBOR::Map(mb)) = is_map()(tb.into_iter())? {
                match mb.get_int(2) {
                    Some(cbor) => match cbor {
                        CBOR::UInt(error_code) => {
                            assert_eq!(error_code, GPP_ROT13_ERROR_OTHER as u64)
                        }
                        _ => assert!(false),
                    },
                    None => assert!(false),
                }
            }
        } else {
            assert!(false)
        }
    }
    Ok(())
}
