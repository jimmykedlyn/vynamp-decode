# FLAC

Public specification consulted for `vynamp-decode-flac`. No libFLAC, FFmpeg,
Symphonia or other third-party code was consulted or copied; the implementation
was written from the specification only.

- IETF RFC 9639, "Free Lossless Audio Codec (FLAC)" (December 2024): the `fLaC`
  stream marker, metadata block header (last-block flag, 7-bit type, 24-bit
  length), STREAMINFO layout (16/16-bit block sizes, 24/24-bit frame sizes,
  20-bit sample rate, 3-bit channels minus one, 5-bit bits per sample minus one,
  36-bit total samples, 128-bit MD5), SEEKTABLE entries (18 bytes, placeholder
  sample number 0xFFFFFFFFFFFFFFFF), and the block types PADDING, APPLICATION,
  VORBIS_COMMENT, CUESHEET and PICTURE. Type 127 is forbidden and types 7 to
  126 are reserved.
- ID3v2 tags in front of the `fLaC` marker are tolerated (skipped by the core
  `skip_id3v2`); they are not part of RFC 9639.
