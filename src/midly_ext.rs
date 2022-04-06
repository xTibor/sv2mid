use midly::{MidiMessage, TrackEventKind};

pub trait TrackEventKindExt {
    fn is_note_on(&self) -> bool;

    fn is_note_off(&self) -> bool;
}

impl TrackEventKindExt for TrackEventKind<'_> {
    fn is_note_on(&self) -> bool {
        matches!(
            self,
            TrackEventKind::Midi {
                message: MidiMessage::NoteOn { .. },
                ..
            }
        )
    }

    fn is_note_off(&self) -> bool {
        matches!(
            self,
            TrackEventKind::Midi {
                message: MidiMessage::NoteOff { .. },
                ..
            }
        )
    }
}
