# Security

agulha parses untrusted input. Report vulnerabilities privately through GitHub private vulnerability reporting
(Security -> Report a vulnerability) rather than in public issues.

Design guarantees: no `unsafe` in library crates, explicit `Limits` on bytes, frames,
channels, rate, metadata size and container recursion depth, and fuzzing of every
demuxer and decoder before each release.
