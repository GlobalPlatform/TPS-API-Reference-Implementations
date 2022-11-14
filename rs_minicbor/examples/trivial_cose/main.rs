/***************************************************************************************************
 * Copyright (c) 2021, 2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
/***************************************************************************************************
 * Trivial COSE
 *
 * An example of using rs_minicbor to encode, sign, decode and verify COSE_Sign1 structures, using
 * The example in RFC 9052 Appendix C.2.1.
 * Note that the "expected" signature in the RFC requires the use of a deterministic random seed
 * in ECDSA (following RFC 6979). This implementation doesn't support RFC6979, as the underlying
 * crate takes the very sensible, but inconvenient (when a deterministic result is needed), approach
 * of generating a random seed itself.
 * The signature does verify correctly, and intermediate values are as expected.
 **************************************************************************************************/
extern crate crypto_bigint;
extern crate rfc6979;
extern crate ring;
extern crate rs_minicbor;
extern crate sha2;

use crypto_bigint::{ArrayEncoding, U256};
use ring::signature;
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use ring::test::rand::FixedSliceRandom;
use sha2::{Digest, Sha256};
// Needed for deterministic nonces in test cases
use std::error::Error;
use std::io;
use std::io::{Write};

use rs_minicbor::debug::print_hex;
use rs_minicbor::decoder::{
    is_array, is_bstr, is_map, is_tag_with_value, CBORDecoder, SequenceBuffer,
};
use rs_minicbor::debug::Diag;
use rs_minicbor::encoder::*;
use rs_minicbor::error::CBORError;
use rs_minicbor::types::*;

// Keys constants for kid '11' from https://github.com/cose-wg/Examples/blob/master/KeySet.txt,
// kid '11' public key in uncompressed form per SEC1, v2.0
const KID_11_PUB: [u8; 65] = [
    0x4, // x
    0xba, 0xc5, 0xb1, 0x1c, 0xad, 0x8f, 0x99, 0xf9, 0xc7, 0x2b, 0x05, 0xcf, 0x4b, 0x9e, 0x26, 0xd2,
    0x44, 0xdc, 0x18, 0x9f, 0x74, 0x52, 0x28, 0x25, 0x5a, 0x21, 0x9a, 0x86, 0xd6, 0xa0, 0x9e, 0xff,
    // y
    0x20, 0x13, 0x8b, 0xf8, 0x2d, 0xc1, 0xb6, 0xd5, 0x62, 0xbe, 0x0f, 0xa5, 0x4a, 0xb7, 0x80, 0x4a,
    0x3a, 0x64, 0xb6, 0xd7, 0x2c, 0xcf, 0xed, 0x6b, 0x6f, 0xb6, 0xed, 0x28, 0xbb, 0xfc, 0x11, 0x7e,
];
// kid '11' Private key
//const KID_11_PRIV: [u8;32] = [
//    0x57, 0xc9, 0x20, 0x77, 0x66, 0x41, 0x46, 0xe8, 0x76, 0x76, 0x0c, 0x95, 0x20, 0xd0, 0x54, 0xaa,
//    0x93, 0xc3, 0xaf, 0xb0, 0x4e, 0x30, 0x67, 0x05, 0xdb, 0x60, 0x90, 0x30, 0x85, 0x07, 0xb4, 0xd3
//];
const KID_11_PRIV: U256 =
    U256::from_be_hex("57c92077664146e876760c9520d054aa93c3afb04e306705db6090308507b4d3");
// NIST P256 curve modulus
const NIST_P256_MODULUS: U256 =
    U256::from_be_hex("FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551");

fn print_bytes(s: &str, buf: &SequenceBuffer) {
    print!("{} ", s);
    for byte in buf.bytes {
        print!("{}", print_hex(*byte))
    }
    println!();
}

fn dup_from_slice(src: &[u8], dest: &mut Vec<u8>) {
    for i in src {
        dest.push(*i);
    }
}

// Generate a value of 'k' to be used in ECDSA, following the specification in RFC6979.
fn rfc6979drbg(priv_key: &[u8], msg: &[u8], aad: &[u8], rnd_bytes: &mut [u8; 32]) {
    let key = U256::from_be_slice(priv_key);
    let h = Sha256::digest(msg);
    let k = rfc6979::generate_k::<Sha256, U256>(&key, &NIST_P256_MODULUS, &h, &aad);
    let k = k.to_be_byte_array();
    let k = k.as_slice();
    for i in 0..k.len() {
        rnd_bytes[i] = k[i];
    }
}

// Generate the COSE_Sign1 "to be signed" structure defined in RFC9052 Section 4.4. This is
// required for both signing and verifying
fn construct_to_be_signed<'a>(
    protected: &mut CBORBuilder,
    payload: &[u8],
    to_be_signed: &'a mut CBORBuilder,
) -> Result<SequenceBuffer<'a>, Box<dyn Error>> {
    Ok(to_be_signed
        // Outer array of Sig_Struct
        .insert(&array(|sig_struct| {
            sig_struct
                // Context
                .insert(&"Signature1")?
                // Body protected
                .insert(&protected.build()?.bytes)?
                // Sign protected - not present
                // External AAD: ''
                .insert(&b"".as_slice())?
                // Payload
                .insert(&payload)
        }))?
        .build()?)
}

// Perform a COSE_Sign1 operation on `payload` into `enc_buf`. It is assumed that `enc_buf`
// already has the unprotected and protected headers encoded, but a reference to the protected
// headers is required as they form part of the content to be signed.
//
// Note: This implementation is delibertely simplified and does not include any support for
//       `aad`, which normally would be part of the operation.
fn cose_sign1<'a>(
    enc_buf: &mut EncodeBuffer<'a>,
    protected: &mut CBORBuilder,
    payload: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut to_be_signed_buf: [u8; 256] = [0; 256];

    // What we are going to sign (from RFC9052, Section 4.4)
    let mut to_be_signed = CBORBuilder::new(&mut to_be_signed_buf);

    let to_be_signed = construct_to_be_signed(protected, payload, &mut to_be_signed)?;
    print_bytes("To be signed", &to_be_signed);

    // Generate the signature
    if let Ok(sign_key) = EcdsaKeyPair::from_private_key_and_public_key(
        &ECDSA_P256_SHA256_FIXED_SIGNING,
        &KID_11_PRIV.to_be_byte_array(),
        &KID_11_PUB,
    ) {
        // Generate deterministic random bytes for ECDSA
        let mut deterministic_bytes: [u8; 32] = [0; 32];
        rfc6979drbg(
            &KID_11_PRIV.to_be_byte_array(),
            &to_be_signed.bytes,
            &[],
            &mut deterministic_bytes,
        );
        let rng = FixedSliceRandom {
            bytes: deterministic_bytes.as_slice(),
        };

        if let Ok(signature) = sign_key.sign(&rng, &to_be_signed.bytes) {
            enc_buf.insert(&signature.as_ref())?;

            Ok(())
        } else {
            Err(CBORError::NotAllowed)?
        }
    } else {
        Err(CBORError::MalformedEncoding)?
    }
}

// Perform a verify operation on a COSE_Sign1 structure
fn cose_verify1(
    protected: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut to_be_verified_buf: [u8; 256] = [0; 256];
    let mut buf: [u8;64] = [0;64];
    let mut protected_bldr = CBORBuilder::new(&mut buf);
    // Problem
    let protected_bldr = protected_bldr.insert_cbor(protected)?;

    let mut to_be_verified = CBORBuilder::new(&mut to_be_verified_buf);
    let to_be_verified = construct_to_be_signed(protected_bldr,
        payload,
        &mut to_be_verified,
    )?;
    print_bytes("To be verified", &to_be_verified);

    let pub_key =
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &KID_11_PUB);
    let result = pub_key.verify(to_be_verified.bytes, signature);
    match result {
        Ok(()) => Ok(()),
        Err(_) => Err(CBORError::MalformedEncoding)?,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut fp = io::stdout();

    let mut enc_buf: [u8; 256] = [0; 256];
    // Construct the COSE_Sign1 structure
    let mut enc_struct = CBORBuilder::new(&mut enc_buf);
    let enc_struct = enc_struct.insert(&tag(18, |enc_struct| {
        enc_struct.insert(&array(|sign1| {
            // Protected headers: << { alg: ECDSA256 } >>
            let mut ph_buf: [u8; 64] = [0; 64];
            let mut prot_hdrs = CBORBuilder::new(&mut ph_buf);
            let protected_headers =
                prot_hdrs.insert(&map(|protected| protected.insert_key_value(&1, &(-7))))?;

            // payload: 'This is the content.'
            let payload = b"This is the content.".as_slice();

            // Construct the first array entries
            sign1
                // / protected / h''
                .insert(&protected_headers.build()?.bytes)?
                // / unprotected / {kid: '11'}
                .insert(&map(|unprotected| {
                    unprotected.insert_key_value(&2, &b"11".as_slice())
                }))?
                // / payload / "This is the content."
                .insert(&payload)?;

            // / signatures /
            match cose_sign1(sign1, protected_headers, &payload) {
                Ok(()) => Ok(sign1),
                Err(_) => Err(CBORError::MalformedEncoding),
            }
        }))
    }))?;

    let bytes = enc_struct.build()?;
    print_bytes("Output", &bytes);
    // Diagnostic output
    bytes.cbor_diag(&mut fp)?;
    fp.flush()?;


    // Verify a COSE_Sign1 structure
    let mut verifier = CBORDecoder::new(bytes);
    let mut alg = 0;
    let mut kid: [u8; 2] = [0; 2];
    let mut protected_hdrs = Vec::<u8>::new();
    let mut payload = Vec::<u8>::new();
    let mut signature = Vec::<u8>::new();
    let mut tag: u64 = 18;

    // Extract the critical bits of the COSE Sign1 structure
    let _r = verifier
        .decode_with(is_tag_with_value(tag), |tagged| {
            CBORDecoder::from_tag(tagged, &mut tag)?
                .decode_with(is_array(), |array| {
                    CBORDecoder::from_array(array)?
                        // Protected Headers
                        .decode_with(is_bstr(), |empty_or_cbor| {
                            if let CBOR::Bstr(bs) = empty_or_cbor {
                                // Make a copy of protected_hdrs
                                if bs.len() > 0 {
                                    // bstr containing CBOR
                                    let mut prot = CBORDecoder::from_slice(bs);
                                    prot.decode_with(is_map(), |map| {
                                        // The CBOR contains a map, but we want the serialized map
                                        // to be duplicated into the protected headers field of the
                                        // to_be_verified structure
                                        if let CBOR::Map(mb) = map {
                                            dup_from_slice(bs, &mut protected_hdrs);
                                            if let Some(alg_v @ CBOR::NInt(_)) = mb.get_int(1) {
                                                alg = alg_v.try_into_i32()?;
                                                return Ok(());
                                            }
                                        }
                                        Err(CBORError::MalformedEncoding)
                                    })?
                                    .finalize()
                                } else {
                                    Err(CBORError::MalformedEncoding)
                                }
                            } else {
                                Err(CBORError::ExpectedType("Should be a bstr here"))
                            }
                        })?
                        // Unprotected Headers
                        .decode_with(is_map(), |map| {
                            if let CBOR::Map(mb) = map {
                                if let Some(CBOR::Bstr(kid_v)) = mb.get(&CBOR::UInt(4)) {
                                    let _ = &kid.copy_from_slice(&kid_v[0..1]);
                                }
                            }
                            Ok(())
                        })?
                        // Payload
                        .decode_with(is_bstr(), |pay| {
                            dup_from_slice(pay.try_into_u8slice()?, &mut payload);
                            Ok(())
                        })?
                        // Signature
                        .decode_with(is_bstr(), |sig| {
                            dup_from_slice(sig.try_into_u8slice()?, &mut signature);
                            Ok(())
                        })?
                        .finalize()
                })?
                .finalize()
        })?
        .finalize();

    // Verify the signature and extracted values
    match cose_verify1(protected_hdrs.as_slice(), payload.as_slice(), signature.as_slice()) {
        Ok(()) => println!("Verification succeeded: message content {:?}", payload),
        Err(_) => println!("Verification failed"),
    }

    Ok(())
}
