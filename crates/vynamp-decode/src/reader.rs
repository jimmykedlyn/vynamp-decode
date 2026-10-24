//! Gapless-aware reader over the first track of a container.

use vynamp_decode_core::{
    AudioBuffer, CancelToken, Decoder, Demuxer, Error, Event, Options, Packet, Registry, Result,
    Source, Track,
};

/// Capacity of the diagnostic event queue.
const MAX_EVENTS: usize = 4096;

/// Decodes the first track of a container, trimming encoder delay, encoder
/// padding and seek preroll so that the output timeline starts at the first
/// audible frame.
pub struct Reader {
    demuxer: Box<dyn Demuxer>,
    decoder: Box<dyn Decoder>,
    track: Track,
    cancel: CancelToken,
    packet: Packet,
    /// Demuxer-timeline position of the next frame to be decoded.
    pos: u64,
    /// Frames below this demuxer position are discarded.
    skip_until: u64,
    /// End of the declared length on the demuxer timeline (if known).
    end: Option<u64>,
    /// Frames at or beyond `end` are discarded. Only done when the track declares
    /// padding: a bare declared length (WAV, FLAC STREAMINFO) can be wrong, and
    /// dropping real audio on its word would hide data.
    enforce_end: bool,
    /// Next frame index on the trimmed output timeline.
    out_pos: u64,
    events: Vec<Event>,
    events_dropped: u64,
}

impl Reader {
    /// Probes `src` with `registry` and prepares to decode track 0.
    ///
    /// `hint_ext` (a file extension without the dot) breaks probe ties.
    pub fn open_with(
        registry: &Registry,
        mut src: Box<dyn Source>,
        opts: Options,
        hint_ext: Option<&str>,
    ) -> Result<Self> {
        opts.cancel.check()?;
        let container = registry.probe(src.as_mut(), hint_ext, &opts.limits)?;
        let demuxer = (container.open)(src, &opts)?;
        let track = demuxer
            .tracks()
            .first()
            .cloned()
            .ok_or(Error::Unsupported("no audio track"))?;
        let codec = registry
            .codec_for(&track.codec)
            .ok_or(Error::Unsupported("codec"))?;
        let decoder = (codec.make)(&track, &opts)?;
        let delay = u64::from(track.delay);
        let end = track
            .frames
            .map(|f| f.saturating_sub(u64::from(track.padding)).max(delay));
        let mut reader = Self {
            demuxer,
            decoder,
            cancel: opts.cancel,
            packet: Packet::default(),
            pos: 0,
            skip_until: delay,
            end,
            enforce_end: track.padding > 0,
            out_pos: 0,
            events: Vec::new(),
            events_dropped: 0,
            track,
        };
        reader.collect_events();
        Ok(reader)
    }

    /// Moves pending demuxer and decoder events into the queue, dropping and
    /// counting those beyond [`MAX_EVENTS`].
    fn collect_events(&mut self) {
        let mut fresh = Vec::new();
        self.demuxer.drain_events(&mut fresh);
        self.decoder.drain_events(&mut fresh);
        let room = MAX_EVENTS.saturating_sub(self.events.len());
        if fresh.len() > room {
            self.events_dropped += (fresh.len() - room) as u64;
            fresh.truncate(room);
        }
        self.events.append(&mut fresh);
    }

    /// The track being decoded.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// Length of the trimmed output in frames (declared frames minus delay and
    /// padding, saturating), or `None` if the track length is unknown.
    ///
    /// The bound is enforced only for tracks that declare padding; otherwise the
    /// declared length is a lower bound: a stream with more audio than it declared
    /// (reported as `Event::Clamped` in `Mode::Resilient`) is delivered in full.
    pub fn frames(&self) -> Option<u64> {
        self.end
            .map(|e| e.saturating_sub(u64::from(self.track.delay)))
    }

    /// Index of the next output frame on the trimmed timeline; updated by
    /// [`read`](Self::read) and [`seek`](Self::seek).
    pub fn position(&self) -> u64 {
        self.out_pos
    }

    /// Takes the diagnostic events reported so far (recoveries made in
    /// `Mode::Resilient`), leaving the queue empty and resetting
    /// [`events_dropped`](Self::events_dropped).
    ///
    /// The queue holds at most 4096 events; call this periodically on long or
    /// badly damaged streams, as further events are dropped (and counted).
    pub fn take_events(&mut self) -> Vec<Event> {
        self.events_dropped = 0;
        std::mem::take(&mut self.events)
    }

    /// Number of events dropped because the queue was full since the last
    /// [`take_events`](Self::take_events).
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    /// Decodes the next chunk into `out`.
    ///
    /// Returns `Ok(true)` with at least one frame in `out`, or `Ok(false)` at
    /// the end of the stream, in which case `out` is left empty.
    pub fn read(&mut self, out: &mut AudioBuffer) -> Result<bool> {
        let r = self.read_inner(out);
        // Events are collected on every path, including errors and end of stream.
        self.collect_events();
        r
    }

    fn read_inner(&mut self, out: &mut AudioBuffer) -> Result<bool> {
        loop {
            self.cancel.check()?;
            if self.end.is_some_and(|e| self.enforce_end && self.pos >= e) {
                out.truncate_back(out.frames());
                return Ok(false);
            }
            if !self.demuxer.next_packet(&mut self.packet)? {
                out.truncate_back(out.frames());
                return Ok(false);
            }
            self.decoder.decode(&self.packet, out)?;
            // The demuxer timeline is authoritative: a packet that decoded to
            // fewer (or more) frames than it covers must not shift later ones.
            self.pos = self.packet.pts;
            let start = self.pos;
            let n = out.frames() as u64;
            self.pos = start.saturating_add(n);
            let front = self.skip_until.saturating_sub(start).min(n);
            let back = self
                .end
                .filter(|_| self.enforce_end)
                .map_or(0, |e| self.pos.saturating_sub(e))
                .min(n - front);
            out.truncate_back(back as usize);
            out.truncate_front(front as usize);
            if out.frames() > 0 {
                self.out_pos = self.out_pos.saturating_add(out.frames() as u64);
                return Ok(true);
            }
        }
    }

    /// Seeks to `frame` on the trimmed output timeline.
    pub fn seek(&mut self, frame: u64) -> Result<()> {
        let target = frame.saturating_add(u64::from(self.track.delay));
        let to = self.demuxer.seek(self.track.id, target)?;
        self.decoder.reset();
        self.pos = to.frame;
        self.out_pos = match self.frames() {
            Some(len) if self.enforce_end => frame.min(len),
            _ => frame,
        };
        self.skip_until = to
            .frame
            .saturating_add(to.preroll)
            .max(u64::from(self.track.delay));
        Ok(())
    }
}
