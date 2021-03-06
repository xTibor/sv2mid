#![feature(io_read_to_string)]

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use midly::num::{u15, u24, u28, u4, u7};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind,
};

mod utils;
use crate::utils::{parse_positive_literal, Seconds};

mod sv_model;
use crate::sv_model::SvDocument;

mod midly_ext;
use crate::midly_ext::TrackEventKindExt;

const MIDI_DRUM_CHANNEL: u8 = 9;

const MIDI_VELOCITY_DEFAULT: u8 = 64;
const MIDI_VELOCITY_NONE: u8 = 0;

const MIDI_CONTROLLER_VOLUME: u8 = 7;
const MIDI_CONTROLLER_PAN: u8 = 10;

const MIDI_MAX_POLYPHONY: usize = 24;

/// A less broken MIDI-exporter for Sonic Visualiser
#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    /// Input project file path
    sv_input_path: PathBuf,

    /// Converted MIDI file path
    midi_output_path: PathBuf,

    /// Fixed MIDI tempo used for exporting
    #[clap(short = 't', long, default_value = "120.0", parse(try_from_str = parse_positive_literal))]
    midi_bpm: f64,

    /// Number of MIDI ticks per beat
    #[clap(short = 'x', long, default_value = "1024", parse(try_from_str = parse_positive_literal))]
    midi_ticks_per_beat: usize,

    /// Trim the leading silence before the first note
    #[clap(short = 's', long)]
    trim_leading_silence: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let sv_document = SvDocument::load(&args.sv_input_path)?;

    if sv_document.get_layers_by_type("notes").count() > 15 {
        eprintln!("warning: project has more notes layers than available MIDI channels");
        eprintln!("note: unassignable layers will be dropped");
    }

    let sv_notes_layers = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15]
        .into_iter()
        .map(u4::from)
        .zip(sv_document.get_layers_by_type("notes"))
        .collect::<Vec<_>>();

    let sv_instants_layers = sv_document
        .get_layers_by_type("timeinstants")
        .collect::<Vec<_>>();

    let sv_text_layers = sv_document.get_layers_by_type("text").collect::<Vec<_>>();

    let mut midi_document = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::from(args.midi_ticks_per_beat as u16)),
    ));

    let mut midi_track = Track::new();

    // MIDI track initialization
    {
        assert!(args.midi_bpm > 0.0);

        midi_track.push(TrackEvent {
            delta: u28::from(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from(
                (60_000_000.0 / args.midi_bpm) as u32,
            ))),
        });

        for &(channel, notes_layer) in sv_notes_layers.iter() {
            {
                if !notes_layer.midi_name().is_ascii() {
                    eprintln!(
                        "warning: non-ASCII instrument name '{}'",
                        notes_layer.midi_name().escape_default(),
                    );
                    eprintln!(
                        "note: these instrument names may be mishandled by other music software"
                    );
                }

                midi_track.push(TrackEvent {
                    delta: u28::from(0),
                    kind: TrackEventKind::Meta(MetaMessage::MidiChannel(channel)),
                });

                midi_track.push(TrackEvent {
                    delta: u28::from(0),
                    kind: TrackEventKind::Meta(MetaMessage::InstrumentName(
                        notes_layer.midi_name().as_bytes(),
                    )),
                });
            }

            let play_parameters = sv_document
                .get_play_parameters_by_id(notes_layer.model)
                .expect("failed to find play parameters");

            midi_track.push(TrackEvent {
                delta: u28::from(0),
                kind: TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::ProgramChange {
                        program: play_parameters.midi_program(),
                    },
                },
            });

            if play_parameters.mute {
                midi_track.push(TrackEvent {
                    delta: u28::from(0),
                    kind: TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::Controller {
                            controller: u7::from(MIDI_CONTROLLER_VOLUME),
                            value: u7::from(0),
                        },
                    },
                });
            } else {
                // TODO: play_parameters.gain
                // Input range: 0.0-4.0, default 1.0
                // MIDI range: 0-127, default 100
            }

            midi_track.push(TrackEvent {
                delta: u28::from(0),
                kind: TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::Controller {
                        controller: u7::from(MIDI_CONTROLLER_PAN),
                        value: u7::from((64.0 + (play_parameters.pan * 63.5)) as u8),
                    },
                },
            });
        }

        // TODO: Drum channel initialization
        // The drum channel is constructed by merging multiple time instant
        // layers. It's not obvious how should channel volume/panning be
        // initialized. I'm leaving it as default for now.
    }

    // Emitting MIDI track data
    {
        struct AbsoluteTrackEvent<'a> {
            /// Absolute MIDI position of the event.
            ticks: usize,

            /// Absolute MIDI position when the note/event has actually been
            /// started (the corresponding NoteOn event for NoteOff events).
            /// Only used as an additional sorting key when preparing events for
            /// delta-encoding and linting (overlaps, excessive polyphony).
            ///
            /// This field has been introduced for properly resolving that case
            /// when a note stops at the same moment when a new one starts.
            /// Event sorting must ensure that the NoteOn event of Note#2 must
            /// not preceed the NoteOff event of Note#1 for obvious reasons.
            /// ```
            /// Time   |-1- - - - -2- - - - -3-|
            ///        |           V           |
            /// Note#1 | [=========]           |
            /// Note#2 |           [=========] |
            /// ```
            ticks_event_start: usize,

            /// The position of the event in seconds, used for error reporting.
            /// This field has been introduced because the "Sonic Visualiser
            /// seconds"->"MIDI ticks" conversion is lossy and caused extreme
            /// precision loss at the error message timestamps in some cases.
            seconds: Seconds,

            /// MIDI event data.
            kind: TrackEventKind<'a>,
        }

        let mut absolute_track_events = Vec::new();

        absolute_track_events.extend(sv_notes_layers.iter().flat_map(|&(channel, notes_layer)| {
            let model = sv_document
                .get_model_by_id(notes_layer.model)
                .expect("notes layer doesn't have model specified");

            let dataset_id = model.dataset.expect("model doesn't have dataset specified");
            let dataset = sv_document
                .get_dataset_by_id(dataset_id)
                .expect("dataset doesn't exist");

            dataset.points.iter().flat_map(move |point| {
                let key = point
                    .value
                    .expect("notes layer point has no value specified");

                let duration = point
                    .duration
                    .expect("notes layer point has no duration specified");

                let seconds_note_on = Seconds::new(point.frame, model.sample_rate);
                let seconds_note_off = Seconds::new(point.frame + duration, model.sample_rate);

                let ticks_note_on = seconds_note_on.as_midi_ticks(args.midi_bpm, args.midi_ticks_per_beat);
                let ticks_note_off = seconds_note_off.as_midi_ticks(args.midi_bpm, args.midi_ticks_per_beat);
                assert!(ticks_note_on <= ticks_note_off);

                // There's a bug in Sonic Visualiser when accidentally right clicking
                // while drawing notes it creates an additional collapsed note next to the
                // drawn note. These collapsed notes fuck up MIDI import in DAWs.
                // Just warn about these issues, better fix them in the source project
                // than here.
                if duration <= 1 {
                    eprintln!(
                        "warning: collapsed note on notes layer '{}' at {}",
                        notes_layer.midi_name().escape_default(),
                        seconds_note_on
                    );
                }

                if ticks_note_on == ticks_note_off {
                    eprintln!(
                        "warning: insufficient resolution to represent MIDI note on notes layer '{}' at {}",
                        notes_layer.midi_name().escape_default(),
                        seconds_note_on
                    );
                }

                [
                    // Note on event
                    AbsoluteTrackEvent {
                        ticks: ticks_note_on,
                        ticks_event_start: ticks_note_on,
                        seconds: seconds_note_on,
                        kind: TrackEventKind::Midi {
                            channel,
                            message: MidiMessage::NoteOn {
                                key: u7::from(key as u8),
                                vel: u7::from(MIDI_VELOCITY_DEFAULT),
                            },
                        },
                    },
                    // Note off event
                    AbsoluteTrackEvent {
                        ticks: ticks_note_off,
                        ticks_event_start: ticks_note_on, // Not a typo
                        seconds: seconds_note_off,
                        kind: TrackEventKind::Midi {
                            channel,
                            message: MidiMessage::NoteOff {
                                key: u7::from(key as u8),
                                vel: u7::from(MIDI_VELOCITY_NONE),
                            },
                        },
                    },
                ]
            })
        }));

        absolute_track_events.extend(sv_instants_layers.iter().flat_map(|&instants_layer| {
            let model = sv_document
                .get_model_by_id(instants_layer.model)
                .expect("instants layer doesn't have model specified");

            let dataset_id = model.dataset.expect("model doesn't have dataset specified");
            let dataset = sv_document
                .get_dataset_by_id(dataset_id)
                .expect("dataset doesn't exist");

            let play_parameters = sv_document
                .get_play_parameters_by_id(instants_layer.model)
                .expect("failed to find play parameters");

            let key = play_parameters.midi_drum_note();

            dataset.points.iter().flat_map(move |point| {
                let seconds_note_on = Seconds::new(point.frame, model.sample_rate);

                assert!(args.midi_ticks_per_beat > 0);
                let length_ticks = args.midi_ticks_per_beat / 4; // Expand the zero-length instants into 1/32 MIDI notes

                let ticks_note_on = seconds_note_on.as_midi_ticks(args.midi_bpm, args.midi_ticks_per_beat);
                let ticks_note_off = ticks_note_on + length_ticks;
                assert!(ticks_note_on <= ticks_note_off);

                if ticks_note_on == ticks_note_off {
                    eprintln!(
                        "warning: insufficient resolution to represent MIDI note on instants layer '{}' at {}",
                        instants_layer.midi_name().escape_default(),
                        seconds_note_on
                    );
                }

                [
                    // Note on event
                    AbsoluteTrackEvent {
                        ticks: ticks_note_on,
                        ticks_event_start: ticks_note_on,
                        seconds: seconds_note_on,
                        kind: TrackEventKind::Midi {
                            channel: u4::from(MIDI_DRUM_CHANNEL),
                            message: MidiMessage::NoteOn {
                                key,
                                vel: u7::from(MIDI_VELOCITY_DEFAULT),
                            },
                        },
                    },
                    // Note off event
                    AbsoluteTrackEvent {
                        ticks: ticks_note_off,
                        ticks_event_start: ticks_note_on, // Not a typo
                        seconds: seconds_note_on, // Instants are zero-length, this is okay.
                        kind: TrackEventKind::Midi {
                            channel: u4::from(MIDI_DRUM_CHANNEL),
                            message: MidiMessage::NoteOff {
                                key,
                                vel: u7::from(MIDI_VELOCITY_NONE),
                            },
                        },
                    },
                ]
            })
        }));

        absolute_track_events.extend(sv_text_layers.iter().flat_map(|&text_layer| {
            let model = sv_document
                .get_model_by_id(text_layer.model)
                .expect("text layer doesn't have model specified");

            let dataset_id = model.dataset.expect("model doesn't have dataset specified");
            let dataset = sv_document
                .get_dataset_by_id(dataset_id)
                .expect("dataset doesn't exist");

            dataset.points.iter().map(move |point| {
                let seconds_text = Seconds::new(point.frame, model.sample_rate);

                let ticks_text =
                    seconds_text.as_midi_ticks(args.midi_bpm, args.midi_ticks_per_beat);

                if !point.label.is_ascii() {
                    eprintln!(
                        "warning: non-ASCII label '{}' on text layer '{}' at {}",
                        point.label.escape_default(),
                        text_layer.midi_name().escape_default(),
                        seconds_text
                    );
                    eprintln!("note: these text events may be mishandled by other music software");
                }

                AbsoluteTrackEvent {
                    ticks: ticks_text,
                    ticks_event_start: ticks_text,
                    seconds: seconds_text,
                    kind: TrackEventKind::Meta(MetaMessage::Text(point.label.as_bytes())),
                }
            })
        }));

        absolute_track_events.sort_by_key(
            |&AbsoluteTrackEvent {
                 ticks,
                 ticks_event_start,
                 kind,
                 ..
             }| {
                // TODO: This sorting key is not exhaustive, may cause reproducibility issues
                (
                    ticks,
                    ticks_event_start,
                    !kind.is_note_on(),
                    !kind.is_note_off(),
                )
            },
        );

        {
            let mut current_polyphony = 0;
            let mut already_warned = false;

            for event in absolute_track_events.iter() {
                if event.kind.is_note_on() {
                    current_polyphony += 1;

                    if (current_polyphony > MIDI_MAX_POLYPHONY) && !already_warned {
                        eprintln!("warning: excessive polyphony at {}", event.seconds);
                        already_warned = true;
                    }
                }

                if event.kind.is_note_off() {
                    assert!(current_polyphony > 0);
                    current_polyphony -= 1;

                    if (current_polyphony <= MIDI_MAX_POLYPHONY) && already_warned {
                        already_warned = false;
                    }
                }
            }
        }

        {
            let mut current_note_counts = HashMap::new();

            for event in absolute_track_events.iter() {
                if let TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn { key, .. },
                } = event.kind
                {
                    let note_count = current_note_counts.entry((channel, key)).or_insert(0);
                    *note_count += 1;

                    if *note_count >= 2 {
                        eprintln!("warning: note overlap at {}", event.seconds);
                    }
                }

                if let TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOff { key, .. },
                } = event.kind
                {
                    let note_count = current_note_counts
                        .get_mut(&(channel, key))
                        .expect("failed to get note count");

                    assert!(*note_count > 0);
                    *note_count -= 1;

                    if *note_count == 0 {
                        current_note_counts.remove(&(channel, key));
                    }
                }
            }
        }

        for (event_index, event) in absolute_track_events.iter().enumerate() {
            let delta_time = if event_index == 0 {
                if args.trim_leading_silence {
                    0
                } else {
                    event.ticks
                }
            } else {
                let ticks_before = absolute_track_events[event_index - 1].ticks;
                let ticks_current = absolute_track_events[event_index].ticks;
                assert!(ticks_before <= ticks_current);
                ticks_current - ticks_before
            };

            midi_track.push(TrackEvent {
                delta: u28::from(delta_time as u32),
                kind: event.kind,
            });
        }

        midi_track.push(TrackEvent {
            delta: u28::from(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
    }

    midi_document.tracks.push(midi_track);
    midi_document.save(args.midi_output_path)?;

    Ok(())
}
