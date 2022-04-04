use std::error::Error;
use std::fs::File;
use std::io;
use std::path::Path;

use bzip2_rs::DecoderReader;
use midly::num::u7;
use strong_xml::XmlRead;

#[derive(Debug, XmlRead)]
#[xml(tag = "sv")]
pub struct SvDocument {
    #[xml(child = "data")]
    pub data: SvData,

    #[xml(child = "display")]
    pub display: SvDisplay,

    #[xml(child = "selections")]
    pub selections: SvSelections,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "data")]
pub struct SvData {
    #[xml(child = "model")]
    pub models: Vec<SvModel>,

    #[xml(child = "playparameters")]
    pub play_parameters: Vec<SvPlayParameters>,

    #[xml(child = "layer")]
    pub layers: Vec<SvLayer>,

    #[xml(child = "dataset")]
    pub datasets: Vec<SvDataset>,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "model")]
pub struct SvModel {
    #[xml(attr = "id")]
    pub id: usize,

    #[xml(attr = "name")]
    pub name: String,

    #[xml(attr = "sampleRate")]
    pub sample_rate: usize,

    #[xml(attr = "start")]
    pub start: usize,

    #[xml(attr = "end")]
    pub end: usize,

    #[xml(attr = "type")]
    pub r#type: String,

    #[xml(attr = "file")]
    pub file: Option<String>,

    #[xml(attr = "mainModel")]
    pub main_model: Option<bool>,

    #[xml(attr = "dimensions")]
    pub dimensions: Option<usize>,

    #[xml(attr = "resolution")]
    pub resolution: Option<usize>,

    #[xml(attr = "notifyOnAdd")]
    pub notify_on_add: Option<bool>,

    #[xml(attr = "dataset")]
    pub dataset: Option<usize>,

    #[xml(attr = "subtype")]
    pub subtype: Option<String>,

    #[xml(attr = "valueQuantization")]
    pub value_quantization: Option<usize>,

    #[xml(attr = "minimum")]
    pub minimum: Option<usize>,

    #[xml(attr = "maximum")]
    pub maximum: Option<usize>,

    #[xml(attr = "units")]
    pub units: Option<String>,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "playparameters")]
pub struct SvPlayParameters {
    #[xml(attr = "mute")]
    pub mute: bool,

    #[xml(attr = "pan")]
    pub pan: f64,

    #[xml(attr = "gain")]
    pub gain: f64,

    #[xml(attr = "clipId")]
    pub clip_id: String,

    #[xml(attr = "model")]
    pub model: usize,

    #[xml(child = "plugin")]
    pub plugins: Vec<SvPlugin>,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "plugin")]
pub struct SvPlugin {
    #[xml(attr = "identifier")]
    pub identifier: String,

    #[xml(attr = "program")]
    pub program: String,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "dataset")]
pub struct SvDataset {
    #[xml(attr = "id")]
    pub id: usize,

    #[xml(attr = "dimensions")]
    pub dimensions: usize,

    #[xml(child = "point")]
    pub points: Vec<SvPoint>,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "point")]
pub struct SvPoint {
    #[xml(attr = "frame")]
    pub frame: usize,

    #[xml(attr = "value")]
    pub value: Option<usize>,

    #[xml(attr = "duration")]
    pub duration: Option<usize>,

    #[xml(attr = "level")]
    pub level: Option<f64>,

    #[xml(attr = "label")]
    pub label: String,
}

#[derive(Debug, XmlRead)]
#[xml(tag = "layer")]
pub struct SvLayer {
    #[xml(attr = "id")]
    pub id: usize,

    #[xml(attr = "type")]
    pub r#type: String,

    #[xml(attr = "name")]
    pub name: String,

    #[xml(attr = "model")]
    pub model: usize,

    #[xml(attr = "presentationName")]
    pub presentation_name: Option<String>,
    // TODO: Other properties
}

#[derive(Debug, XmlRead)]
#[xml(tag = "display")]
pub struct SvDisplay {
    // stub
}

#[derive(Debug, XmlRead)]
#[xml(tag = "selections")]
pub struct SvSelections {
    // stub
}

impl SvDocument {
    pub fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let mut bzip2_decoder = DecoderReader::new(File::open(path)?);
        let xml_data = io::read_to_string(&mut bzip2_decoder)?;

        Ok(SvDocument::from_str(&xml_data)?)
    }

    pub fn get_layer_by_id(&self, id: usize) -> Option<&SvLayer> {
        self.data.layers.iter().find(|layer| layer.id == id)
    }

    pub fn get_model_by_id(&self, id: usize) -> Option<&SvModel> {
        self.data.models.iter().find(|model| model.id == id)
    }

    pub fn get_dataset_by_id(&self, id: usize) -> Option<&SvDataset> {
        self.data.datasets.iter().find(|dataset| dataset.id == id)
    }

    pub fn get_play_parameters_by_id(&self, id: usize) -> Option<&SvPlayParameters> {
        self.data
            .play_parameters
            .iter()
            .find(|play_parameters| play_parameters.model == id)
    }
}

impl SvPlayParameters {
    pub fn midi_program(&self) -> u7 {
        u7::from(match self.clip_id.as_str() {
            "piano" => 0,
            "elecpiano" => 5,
            "organ" => 17,
            "beep" => 80,
            _ => 0,
        })
    }

    pub fn midi_drum_note(&self) -> u7 {
        u7::from(match self.clip_id.as_str() {
            "bass" => 35,
            "bounce" => 27,
            "clap" => 39,
            "click" => 33,
            "cowbell" => 56,
            "hihat" => 42,
            "kick" => 41,
            "silent" => 0,
            "snare" => 38,
            "stick" => 30,
            "strike" => 49,
            "tap" => 32,
            _ => 0,
        })
    }
}

impl SvLayer {
    pub fn midi_name(&self) -> &str {
        if let Some(presentation_name) = &self.presentation_name {
            presentation_name
        } else {
            &self.name
        }
    }
}
