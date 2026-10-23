# Legal notes

This file is not legal advice. Whoever distributes a build is responsible for
obtaining any patent licences that apply in their jurisdiction.

## Feature policy

- **default**: formats with open specifications or expired patents (PCM, FLAC, ALAC,
  MP1/2/3, AAC-LC, Vorbis, Opus, WavPack, TTA, Musepack, AC-3, DSD to PCM, ...).
- **`encumbered`** (off by default): HE-AAC v1/v2, xHE-AAC/USAC, E-AC-3, TrueHD/MLP,
  DTS/DTS-HD, AC-4, MPEG-H.
- **`reverse-engineered`** (off by default): formats without a public specification,
  such as the WMA family. APE is in `default` only if the licence review confirms a
  clean implementation; otherwise it moves to an optional feature.
- Encumbered and reverse-engineered codecs are built now in the private repository;
  a legal review is a prerequisite only for opening the source.

## Formats without public specification

TAK, OptimFROG, WMA, ATRAC, DST and parts of DTS-HD/APE are built last. A permissively
licensed source (BSD/MIT/public domain) may be studied, with credit in `docs/sources/`.

## `crates-copyleft/`

When the only available source is copyleft (LGPL/GPL), the crate lives in
`crates-copyleft/` with the same licence as that source. It is outside the default
workspace and disabled by default. The core and the default crates stay
`MIT OR Apache-2.0` and never depend on it. No code is copied: it is a
reimplementation, with a record of what was studied.
