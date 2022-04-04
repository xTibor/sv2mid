#![feature(io_read_to_string)]

use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use midly::num::{u15, u24, u28, u4, u7};
use midly::MetaMessage;

mod sv_model;
use crate::sv_model::SvDocument;

const MIDI_TICKS_PER_BEAT: usize = 1024;
const MIDI_DRUM_CHANNEL: usize = 9;
const MIDI_DRUM_NOTE_LENGTH: usize = MIDI_TICKS_PER_BEAT / 4;

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

    let sv_notes_layers = sv_document
        .data
        .layers
        .iter()
        .filter(|layer| layer.r#type == "notes")
        .enumerate()
        .map(|(channel_index, notes_layer)| {
            // Skip drum channel when assigning MIDI channels to notes layers.
            if channel_index < MIDI_DRUM_CHANNEL {
                (channel_index, notes_layer)
            } else {
                (channel_index + 1, notes_layer)
            }
        })
        .collect::<Vec<_>>();

    let sv_instants_layer = sv_document
        .data
        .layers
        .iter()
        .filter(|layer| layer.r#type == "timeinstants")
        .collect::<Vec<_>>();

    let mut midi_document = midly::Smf::new(midly::Header::new(
        midly::Format::SingleTrack,
        midly::Timing::Metrical(u15::from(MIDI_TICKS_PER_BEAT as u16)),
    ));

    let midi_bpm = args.tempo.unwrap_or(120.0);
    let mut midi_track = midly::Track::new();

    // MIDI track initialization
    {
        midi_track.push(midly::TrackEvent {
            delta: u28::from(0),
            kind: midly::TrackEventKind::Meta(MetaMessage::Tempo(u24::from(
                (60_000_000.0 / midi_bpm) as u32,
            ))),
        });

        for &(channel_index, notes_layer) in sv_notes_layers.iter() {
            midi_track.push(midly::TrackEvent {
                delta: u28::from(0),
                kind: midly::TrackEventKind::Meta(midly::MetaMessage::MidiChannel(u4::from(
                    channel_index as u8,
                ))),
            });

            midi_track.push(midly::TrackEvent {
                delta: u28::from(0),
                kind: midly::TrackEventKind::Meta(midly::MetaMessage::InstrumentName(
                    notes_layer.midi_name().as_bytes(),
                )),
            });

            let play_parameters = sv_document
                .get_play_parameters_by_id(notes_layer.model)
                .expect("failed to find play parameters");

            midi_track.push(midly::TrackEvent {
                delta: u28::from(0),
                kind: midly::TrackEventKind::Midi {
                    channel: u4::from(channel_index as u8),
                    message: midly::MidiMessage::ProgramChange {
                        program: play_parameters.midi_program(),
                    },
                },
            });

            if play_parameters.mute {
                midi_track.push(midly::TrackEvent {
                    delta: u28::from(0),
                    kind: midly::TrackEventKind::Midi {
                        channel: u4::from(channel_index as u8),
                        message: midly::MidiMessage::Controller {
                            controller: u7::from(7),
                            value: u7::from(0),
                        },
                    },
                });
            } else {
                // TODO: play_parameters.gain
                // Input range: 0.0-4.0, default 1.0
                // MIDI range: 0-127, default 100
            }

            midi_track.push(midly::TrackEvent {
                delta: u28::from(0),
                kind: midly::TrackEventKind::Midi {
                    channel: u4::from(channel_index as u8),
                    message: midly::MidiMessage::Controller {
                        controller: u7::from(10),
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
        struct AbsoluteMidiEvent {
            channel_index: usize,
            key: usize,
            ticks: usize,
            note_on: bool,
        }

        let seconds_to_ticks = |seconds: f64| -> usize {
            (seconds * (midi_bpm / 60.0) * MIDI_TICKS_PER_BEAT as f64) as usize
        };

        let mut absolute_midi_events = Vec::new();

        absolute_midi_events.extend(sv_notes_layers.iter().flat_map(
            |&(channel_index, notes_layer)| {
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
                        AbsoluteMidiEvent {
                            channel_index,
                            key,
                            ticks: seconds_to_ticks(offset_seconds),
                            note_on: true,
                        },
                        // Note off event
                        AbsoluteMidiEvent {
                            channel_index,
                            key,
                            ticks: seconds_to_ticks(offset_seconds + length_seconds),
                            note_on: false,
                        },
                    ]
                })
            },
        ));

        absolute_midi_events.extend(sv_instants_layer.iter().flat_map(|&instants_layer| {
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

            let key = play_parameters.midi_drum_note().as_int() as usize;

            dataset.points.iter().flat_map(move |point| {
                let offset_seconds = (point.frame as f64) / (model.sample_rate as f64);

                [
                    // Note on event
                    AbsoluteMidiEvent {
                        channel_index: MIDI_DRUM_CHANNEL,
                        key,
                        ticks: seconds_to_ticks(offset_seconds),
                        note_on: true,
                    },
                    // Note off event
                    AbsoluteMidiEvent {
                        channel_index: MIDI_DRUM_CHANNEL,
                        key,
                        ticks: seconds_to_ticks(offset_seconds) + MIDI_DRUM_NOTE_LENGTH,
                        note_on: false,
                    },
                ]
            })
        }));

        absolute_midi_events.sort_by_key(
            |&AbsoluteMidiEvent {
                 channel_index,
                 key,
                 ticks,
                 note_on,
             }| {
                // Rationale behind this sorting key:
                // - ticks: Interleave the channels and sort events by occurrence.
                // - note_on: When multiple events occur at the same time, emit the
                //     note off events first before any new note on events.
                // - channel_index: Make sorting results reproducible.
                // - key: Make sorting results reproducible.
                (ticks, note_on, channel_index, key)
            },
        );

        for (event_index, event) in absolute_midi_events.iter().enumerate() {
            let delta_time = if event_index == 0 {
                if args.trim_leading_silence {
                    0
                } else {
                    event.ticks
                }
            } else {
                absolute_midi_events[event_index].ticks
                    - absolute_midi_events[event_index - 1].ticks
            };

            if event.note_on {
                midi_track.push(midly::TrackEvent {
                    delta: u28::from(delta_time as u32),
                    kind: midly::TrackEventKind::Midi {
                        channel: u4::from(event.channel_index as u8),
                        message: midly::MidiMessage::NoteOn {
                            key: u7::from(event.key as u8),
                            vel: u7::from(64 as u8),
                        },
                    },
                });
            } else {
                midi_track.push(midly::TrackEvent {
                    delta: u28::from(delta_time as u32),
                    kind: midly::TrackEventKind::Midi {
                        channel: u4::from(event.channel_index as u8),
                        message: midly::MidiMessage::NoteOff {
                            key: u7::from(event.key as u8),
                            vel: u7::from(0 as u8),
                        },
                    },
                });
            }
        }

        midi_track.push(midly::TrackEvent {
            delta: u28::from(0),
            kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        });
    }

    midi_document.tracks.push(midi_track);
    midi_document.save(args.midi_output_path)?;

    Ok(())
}
