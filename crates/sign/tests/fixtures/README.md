# Signed OFD samples

Source: the test resources of [ofdrw](https://github.com/ofdrw/ofdrw)
(`ofdrw-sign/src/test/resources/`), the open-source Java OFD library, via
the `sky-ofd` repository's `tests/` directory. ofdrw's license is recorded
there as MIT, which permits redistribution; confirm against the ofdrw
repository before publishing these files outside this repository.

| File | Purpose |
|---|---|
| `SESV4SignDoc.ofd` | GB/T 38540-2020 seal signature (SES v4), the mainstream format |
| `SESV4SignDocAnnotated.ofd` | The same package plus one entry, `Doc_0/Pages/Page_0/Annotation.xml`: a signed file that has no annotation entry and carries an annotation in the old layout (a page file, no entry file). Made here for the web e2e; the signature still verifies |
| `SESV1SignDoc.ofd` | GM/T 0031-2014 seal signature (SES v1) |
| `SESV5SignDoc.ofd` | SES v5 signature |
| `DigitalSign.ofd` | `Type="Sign"` digital signature (bare SM2 r/s) |
| `digital_signer_cert.der` | The SM2 certificate that made the signature in `DigitalSign.ofd` (the certificate inside ofdrw's test key file `USER.p12`) |
| `sign_cert.pem` | A different SM2 test certificate from the same directory: it does **not** match `DigitalSign.ofd`, so it serves as the wrong-certificate case |

## Certificate files (for the PKCS#12 reader and the certificate store)

| File | Password | What it is |
|---|---|---|
| `USER.p12` | `777777` | ofdrw's test key file (same source as the signed samples). PBES1: the key is protected with 3DES and the certificates with RC2-40, the layout Chinese CA tools still write. Its certificate is the one in `digital_signer_cert.der`. |
| `sm2_test.p12` | `changeit` | Generated locally with OpenSSL 3.2 for this repository: an SM2 key and a self-signed certificate, PBES2 with AES-256-CBC, HMAC-SHA256 and a SHA-256 MAC, the modern layout. A throwaway key, published here on purpose; never use it for anything real. |
| `rsa_test.p12` | `changeit` | Generated locally with OpenSSL 3.2: an RSA key. The negative case for a key file that is not SM2. A throwaway key. |
