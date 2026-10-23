# RIFF / WAVE / RF64 / BW64 / Wave64

Public specifications consulted for `agulha-riff`. No third-party code was
consulted or copied; the implementation was written from the format
descriptions only.

- Microsoft, "Multimedia Programming Interface and Data Specifications 1.0"
  (RIFF container, chunk layout, even-size padding, `WAVE` `fmt ` and `data`
  chunks, format tags 1 = PCM and 3 = IEEE float).
- Microsoft, "Multiple Channel Audio Data and WAVE Files" and the
  `WAVE_FORMAT_EXTENSIBLE` / `WAVEFORMATEXTENSIBLE` documentation (format tag
  0xFFFE, valid bits, channel mask, SubFormat GUIDs
  `00000001-0000-0010-8000-00aa00389b71` for PCM and `00000003-...` for float).
- EBU Tech 3306 v1.0 "MBWF / RF64: an extended File Format for Audio" and
  ITU-R BS.2088 (BW64): `RF64`/`BW64` form types, the `ds64` chunk with 64-bit
  RIFF/data sizes and sample count, 0xFFFFFFFF placeholder sizes.
- Sony Wave64 format description (publicly documented GUID-based chunks):
  `riff`, `wave`, `fmt ` and `data` GUIDs, 64-bit chunk sizes that include the
  24-byte chunk header, 8-byte chunk alignment.
