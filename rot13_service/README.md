# ROT13 Service

An implementation of a minimal TPS Service in Rust, in this case
implementing ROT13 ciphering.

> It goes without saying that ROT13 offers absolutely no security
> whatsoever. ROT13 is used here to eliminate any focus on the
> cryptographic aspects of a service.

Messages are defined in [CBOR (RFC8949)](https://www.rfc-editor.org/rfc/rfc8949.html).
We use [CDDL (RFC8610)](https://www.rfc-editor.org/rfc/rfc8610) to define the
allowed message contents. At least some familiarity with CBOR and CDDL is
helpful to understand the following.

## License

`rot13_service` is MIT licensed.

## Supported messages

- `TPS_GetFeatures_Req`: #6.1
- `TPS_GetFeatures_Rsp`: #6.1
- `GPP_ROT13_Encrypt_Req`: #6.10
- `GPP_ROT13_Encrypt_Rsp`: #6.10
- `GPP_ROT13_Decrypt_Req`: #6.11
- `GPP_ROT13_Decrypt_Rsp`: #6.11

The underlying implementations of encryption and decryption in ROT13 are identical, as ROT13
is reversible. It thus really doesn't matter which is called as they do the same thing.

### Service Discovery

 Like all TPS Services, it is required to respond to the standard connector message
 `TPS_GetFeatures_Req`, returning `TPS_GetFeatures_Rsp`.

 For this service, `TPS_GetFeatures_Rsp` returns the following:

- `svc_name`: h'87bae713b08f5e28b9ee4aa6e202440e'
- `login_method`: [0]
- `$$svc_features` //= (128 => [0, 1])   // "encrypt" and "decrypt"

### Encrypt

 `GPP_ROT13_Encrypt_Req` is encoded in CDDL as follows:

 ```cddl
 GPP_ROT13_Encrypt_Req = #6.10 ({
   1 => tstr
})
```

The service responds with `GPP_ROT13_Encrypt_Rsp`, which is encoded as follows:

```cddl
GPP_ROT13_Encrypt_Rsp = #6.10 ({
  (1 => tstr / 2 => uint)
})
```

If map item 1 is present, "encryption" was successful and the `tstr` contains the "encrypted"
payload. If map item 2 is present, "encryption" failed and the `uint` value provides a helpful
error code as follows:

- 1: space character detected
- 2: numeric character detected
- 3: some other symbol detected
- 4: message was too large to process

> As this crate is intended to be able to be used in environments that
> do not support allocators, we use fixed length buffers. In this code,
> the largest string that can be encoded is 255 characters.

### Decrypt

`GPP_ROT13_Decrypt_Req` is encoded in CDDL as follows:

```cddl
GPP_ROT13_Decrypt_Req = #6.11 ({
  1 => tstr
})
```

The service responds with `GPP_ROT13_Encrypt_Rsp`, which is encoded as follows:

```cddl
 GPP_ROT13_Decrypt_Rsp = #6.11 ({
   (1 => tstr / 2 => uint)
})
```

If map item 1 is present, "decryption" was successful and the `tstr` contains the "decrypted"
payload. If map item 2 is present, "decryption" failed and the `uint` value provides a helpful
error code as follows:

- 1: space character detected
- 2: numeric character detected
- 3: some other symbol detected
- 4: message was too large to process

## Tests

There are test cases covering the operation of the service.
