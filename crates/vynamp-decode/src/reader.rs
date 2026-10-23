//! Gapless-aware reader over the first track of a container.

use vynamp_decode_core::{
    AudioBuffer, CancelToken, Decoder, Demuxer, Error, Event, Options, Packet, Registry, Result,
    Source, Track,
};

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
    /// Frames at or beyond this demuxer position are discarded (if known).
    end: Option<u64>,
    /// Next frame index on the trimmed output timeline.
    out_pos: u64,
    events: Vec<Event>,
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
        let mut demuxer = (container.open)(src, &opts)?;
        let track = demuxer
            .tracks()
            .first()
            .cloned()
            .ok_or(Error::Unsupported("no audio track"))?;
        let codec = registry
            .codec_for(&track.codec)
            .ok_or(Error::Unsupported("codec"))?;
        let decoder = (codec.make)(&track, &opts)?;
        let mut events = Vec::new();
        demuxer.drain_events(&mut events);
        let delay = u64::from(track.delay);
        let end = track
            .frames
            .map(|f| f.saturating_sub(u64::from(track.padding)).max(delay));
        Ok(Self {
            demuxer,
            decoder,
            cancel: opts.cancel,
            packet: Packet::default(),
            pos: 0,
            skip_until: delay,
            end,
            out_pos: 0,
            events,
            track,
        })
    }

    /// The track being decoded.
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// Length of the trimmed output in frames (declared frames minus delay and
    /// padding, saturating), or `None` if the track length is unknown.
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
    /// `Mode::Resilient`), leaving the internal list empty.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Decodes the next chunk into `out`.
    ///
    /// Returns `Ok(true)` with at least one frame in `out`, or `Ok(false)` at
    /// the end of the stream, in which case `out` is left empty.
    pub fn read(&mut self, out: &mut AudioBuffer) -> Result<bool> {
        loop {
            self.cancel.check()?;
            if self.end.is_some_and(|e| self.pos >= e) {
                out.truncate_back(out.frames());
                return Ok(false);
            }
            if !self.demuxer.next_packet(&mut self.packet)? {
                out.truncate_back(out.frames());
                return Ok(false);
            }
            let decoded = self.decoder.decode(&self.packet, out);
            self.demuxer.drain_events(&mut self.events);
            self.decoder.drain_events(&mut self.events);
            decoded?;
            // The demuxer timeline is authoritative: a packet that decoded to
            // fewer (or more) frames than it covers must not shift later ones.
            self.pos = self.packet.pts;
            let start = self.pos;
            let n = out.frames() as u64;
            self.pos = start.saturating_add(n);
            let front = self.skip_until.saturating_sub(start).min(n);
            let back = self
                .end
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
        self.out_pos = self.frames().map_or(frame, |len| frame.min(len));
        self.skip_until = to
            .frame
            .saturating_add(to.preroll)
            .max(u64::from(self.track.delay));
        Ok(())
    }
}
