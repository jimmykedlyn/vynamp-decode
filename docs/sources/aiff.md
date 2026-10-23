# AIFF / AIFF-C

Public specifications consulted for `agulha-aiff`. No third-party code was
consulted or copied; the implementation was written from the format
descriptions only.

- Apple Computer, "Audio Interchange File Format: AIFF, A Standard for Sampled
  Sound Files, Version 1.3" (1989): EA IFF 85 `FORM` container with big-endian
  chunk sizes and even-size padding, the `COMM` chunk (channels, frame count,
  sample size, 80-bit extended sample rate) and the `SSND` chunk (`offset`,
  `blockSize`, sample data left-justified in whole-byte containers).
- Apple Computer, "AIFF-C: A Standard for Sampled Sound Files, Version 1"
  (draft, 1991): `AIFC` form type, `FVER` chunk, `COMM` extended with
  `compressionType` and a pstring `compressionName`; the uncompressed types
  `NONE` and `twos`, and the commonly documented variants `sowt`
  (little-endian), `fl32`/`FL32`, `fl64`/`FL64`, `raw `, `in24`, `in32`,
  `42ni` and `23ni`. Compressed types (for example `ulaw`, `alaw`) are reported
  as unsupported.
- IEEE Std 754-2019 / x87 double-extended format definition: 1 sign bit,
  15-bit exponent with bias 16383, 64-bit significand with an explicit integer
  bit (63-bit fraction). Used by `extended::to_u32`, which accepts only exact
  positive integers up to `u32::MAX`.
