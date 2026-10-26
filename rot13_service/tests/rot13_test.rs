/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

extern crate rot13_service;

use rot13_service::{
    message_handler, GPP_ROT13_CIPHERTEXT_KEY, GPP_ROT13_DECRYPT_REQ, GPP_ROT13_DECRYPT_RSP,
    GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_ENCRYPT_RSP, GPP_ROT13_ERROR_NUMERIC, GPP_ROT13_ERROR_OTHER,
    GPP_ROT13_ERROR_SPACE, GPP_ROT13_PLAINTEXT_KEY,
};
use tps_minicbor::{DecodeError, Decoder, EncodeError, EncoderStd, Item, Map};

/// Build a `#6.<msg_id>({ 1 => text })` request into a buffer, returning the buffer and the
/// length of the encoded message within it.
fn build_req(msg_id: u32, key: u32, text: &str) -> Result<([u8; 100], usize), EncodeError> {
    let mut buf = [0u8; 100];
    let len = {
        let mut encoder = EncoderStd::new(&mut buf);
        let mut msg = encoder.tag(msg_id.into())?.map()?;
        msg.insert_key_value(key, text)?;
        msg.end()?;
        encoder.encoded().len()
    };
    Ok((buf, len))
}

/// Decode a `#6.<msg_id>({ ... })` response, returning the response body map. The buffer may
/// have unused trailing bytes, so `Decoder::next` is used rather than `Decoder::decode`.
fn decode_rsp(buf: &[u8], expected_msg_id: u32) -> Result<Map<'_>, DecodeError> {
    let tagged = match Decoder::new(buf).next()? {
        Item::Tag(tagged) => tagged,
        _ => return Err(DecodeError::UnexpectedItem),
    };
    assert_eq!(tagged.tag, expected_msg_id as u64);
    match tagged.item()? {
        Item::Map(map) => Ok(map),
        _ => Err(DecodeError::UnexpectedItem),
    }
}

/// Find the value for `key` in `map`, if present.
fn map_get<'a>(map: &Map<'a>, key: u64) -> Option<Item<'a>> {
    map.iter()
        .find_map(|pair| pair.ok().filter(|(k, _)| matches!(k, Item::Unsigned(v) if *v == key)))
        .map(|(_, v)| v)
}

#[test]
fn test_encode_lowercase_success() -> Result<(), DecodeError> {
    let plaintext = "thequickbrownfoxjumpsoverthelazydog";
    let ciphertext = "gurdhvpxoebjasbkwhzcfbiregurynmlqbt";
    let (send_buf, send_len) = build_req(GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_PLAINTEXT_KEY, plaintext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_ENCRYPT_RSP)?;
    match map_get(&map, 1) {
        Some(Item::Text(receive_text)) => assert_eq!(receive_text, ciphertext),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_encode_uppercase_success() -> Result<(), DecodeError> {
    let plaintext = "THEQUICKBROWNFOXJUMPSOVERTHELAZYDOG";
    let ciphertext = "GURDHVPXOEBJASBKWHZCFBIREGURYNMLQBT";
    let (send_buf, send_len) = build_req(GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_PLAINTEXT_KEY, plaintext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_ENCRYPT_RSP)?;
    match map_get(&map, 1) {
        Some(Item::Text(receive_text)) => assert_eq!(receive_text, ciphertext),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_encode_digit_failure() -> Result<(), DecodeError> {
    let plaintext = "thequickbrownfox9umpsoverthelazydog";
    let (send_buf, send_len) = build_req(GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_PLAINTEXT_KEY, plaintext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_ENCRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_NUMERIC as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_decode_digit_failure() -> Result<(), DecodeError> {
    let ciphertext = "GURDHVPXOEBJ4SBKWHZCFBIREGURYNMLQBT";
    let (send_buf, send_len) = build_req(GPP_ROT13_DECRYPT_REQ, GPP_ROT13_CIPHERTEXT_KEY, ciphertext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_DECRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_NUMERIC as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_encode_space_failure() -> Result<(), DecodeError> {
    let plaintext = "the quick brown fox jumps over the lazy dog";
    let (send_buf, send_len) = build_req(GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_PLAINTEXT_KEY, plaintext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_ENCRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_SPACE as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_decode_space_failure() -> Result<(), DecodeError> {
    let ciphertext = "GUR DHVPX OEBJA SBK WHZCF BIRE GUR YNML QBT";
    let (send_buf, send_len) = build_req(GPP_ROT13_DECRYPT_REQ, GPP_ROT13_CIPHERTEXT_KEY, ciphertext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_DECRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_SPACE as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_encode_other_failure() -> Result<(), DecodeError> {
    let plaintext = "";
    let (send_buf, send_len) = build_req(GPP_ROT13_ENCRYPT_REQ, GPP_ROT13_PLAINTEXT_KEY, plaintext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_ENCRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_OTHER as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}

#[test]
fn test_decode_other_failure() -> Result<(), DecodeError> {
    let ciphertext = "";
    let (send_buf, send_len) = build_req(GPP_ROT13_DECRYPT_REQ, GPP_ROT13_CIPHERTEXT_KEY, ciphertext)
        .expect("encode request");
    let mut receive_buf = [0u8; 100];

    message_handler(&send_buf[..send_len], &mut receive_buf).expect("message_handler");

    let map = decode_rsp(&receive_buf, GPP_ROT13_DECRYPT_RSP)?;
    match map_get(&map, 2) {
        Some(Item::Unsigned(error_code)) => assert_eq!(error_code, GPP_ROT13_ERROR_OTHER as u64),
        other => panic!("unexpected item: {other:?}"),
    }
    Ok(())
}
