# Adding a codec

1. **Study the specification, not other implementations' code.** vynamp-decode is
   clean-room: decoders are written from public specifications. Record every
   specification or reference studied in `docs/sources/` (name, version/date,
   URL, licence, what was learned). Never copy code. Copyleft sources follow the
   policy in `LEGAL.md`.
2. **Write the failing test first.** Lossless codecs are tested bit-exact;
   lossy codecs against official conformance vectors where they exist.
3. **Implement the `Decoder` trait** in its own crate (`vynamp-decode-<codec>`) that
   depends only on `vynamp-decode-core`. Report the full timeline; leave delay and
   padding trimming to the `Reader`, and expose them through `Track`.
4. **Add a `CodecDescriptor`** in the facade, behind a Cargo feature:

   ```rust,ignore
   pub static MYCODEC: CodecDescriptor = CodecDescriptor {
       name: "mycodec",
       verification: Verification::Experimental,
       supports: |id| matches!(id, CodecId::MyCodec),
       make: |track, _| Ok(Box::new(vynamp_decode_mycodec::Decoder::new(track)?)),
   };
   ```

   Register it in `default_registry` with `reg.add_codec(&MYCODEC)`.
5. **Declare the verification level honestly:**

   | Level | Meaning |
   |-------|---------|
   | `Conformance` | Bit-exact against official conformance vectors only; round trips do not qualify. |
   | `Oracle` | Matches a reference implementation's output on a recorded corpus (PCM containers and codec). |
   | `Experimental` | Not yet verified; do not rely on it. |

6. Update the README support table (`cargo run --example support_matrix`) and the
   CHANGELOG.
