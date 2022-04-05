#![feature(io_read_to_string)]

use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use midly::num::{u15, u24, u28, u4, u7};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind,
};

mod sv_model;
use crate::sv_model::SvDocument;

const MIDI_TICKS_PER_BEAT: usize = 1024;

const MIDI_DRUM_CHANNEL: u8 = 9;
const MIDI_DRUM_NOTE_LENGTH: usize = MIDI_TICKS_PER_BEAT / 4;

const MIDI_VELOCITY_DEFAULT: u8 = 64;
const MIDI_VELOCITY_NONE: u8 = 0;

const MIDI_CONTROLLER_VOLUME: u8 = 7;
const MIDI_CONTROLLER_PAN: u8 = 10;

/// A less broken MIDI-exporter for Sonic Visualiser
#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    /// Input project file path
    sv_input_path: PathBuf,

    /// Converted MIDI file path
    midi_output_path: PathBuf,

    /// Fixed MIDI tempo used for exporting
    #[clap(short = 't', long)]
    tempo: Option<f64>,

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
        Timing::Metrical(u15::from(MIDI_TICKS_PER_BEAT as u16)),
    ));

    let midi_bpm = args.tempo.unwrap_or(120.0);
    let mut midi_track = Track::new();

    // MIDI track initialization
    {
        midi_track.push(TrackEvent {
            delta: u28::from(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from(
                (60_000_000.0 / midi_bpm) as u32,
            ))),
        });

        for &(channel, notes_layer) in sv_notes_layers.iter() {
            {
                if !notes_layer.midi_name().is_ascii() {
                    eprintln!(
                        "warning: non-ASCII instrument name '{}'",
                        notes_layer.midi_name(),
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
        // The drum channel is constructed by merging multiple time instant layers.
        // It's not obvious how should channel volume/panning be initialized.
        // I'm leaving it as default for now.
    }

    // Emitting MIDI track data
    {
        struct AbsoluteTrackEvent<'a> {
            ticks: usize,
            kind: TrackEventKind<'a>,
        }

        let seconds_to_ticks = |seconds: f64| -> usize {
            (seconds * (midi_bpm / 60.0) * MIDI_TICKS_PER_BEAT as f64) as usize
        };

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

                let offset_seconds = (point.frame as f64) / (model.sample_rate as f64);
                let length_seconds = (duration as f64) / (model.sample_rate as f64);

                // There's a bug in Sonic Visualiser when accidentally right clicking
                // while drawing notes it creates an additional imploded note next to the
                // drawn note. These imploded notes fuck up MIDI import in DAWs.
                // Just warn about these issues, better fix them in the source project
                // than here.
                if duration <= 1 {
                    eprintln!(
                        "warning: imploded note on layer '{}' at {:.2}s",
                        notes_layer.midi_name(),
                        offset_seconds
                    );
                }

                [
                    // Note on event
                    AbsoluteTrackEvent {
                        ticks: seconds_to_ticks(offset_seconds),
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
                        ticks: seconds_to_ticks(offset_seconds + length_seconds),
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
                let offset_seconds = (point.frame as f64) / (model.sample_rate as f64);

                [
                    // Note on event
                    AbsoluteTrackEvent {
                        ticks: seconds_to_ticks(offset_seconds),
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
                        ticks: seconds_to_ticks(offset_seconds) + MIDI_DRUM_NOTE_LENGTH,
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
                let offset_seconds = (point.frame as f64) / (model.sample_rate as f64);

                if !point.label.is_ascii() {
                    eprintln!(
                        "warning: non-ASCII label '{}' on text layer '{}' at {:.2}s",
                        point.label,
                        text_layer.midi_name(),
                        offset_seconds
                    );
                    eprintln!("note: these text events may be mishandled by other music software");
                }

                AbsoluteTrackEvent {
                    ticks: seconds_to_ticks(offset_seconds),
                    kind: TrackEventKind::Meta(MetaMessage::Text(point.label.as_bytes())),
                }
            })
        }));

        absolute_track_events.sort_by_key(|&AbsoluteTrackEvent { ticks, kind }| {
            let is_note_off_event = matches!(
                kind,
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOff { .. },
                    ..
                }
            );

            let is_note_on_event = matches!(
                kind,
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { .. },
                    ..
                }
            );

            // Sort by time, then NoteOff -> NoteOn -> other events.
            // TODO: This sorting key is not exhaustive, may cause reproducibility issues
            (ticks, !is_note_off_event, !is_note_on_event)
        });

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
