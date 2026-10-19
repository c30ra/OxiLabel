//! Enhancement filters: which one and with which parameters. The viewer's filter panel chooses
//! them and the server computes the filtered pyramid; both name a filter by its [`Filter::key`],
//! which is also the cache folder of its tiles. The filters are those of the author's
//! `defect_detection/seg_prep/common.py`, with the same parameters and defaults.

/// One of the filters, as in `common.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    /// `flatten_illumination`: divides by a heavily blurred copy, removing uneven lighting.
    Flatten,
    /// `enhance_for_tagging`: flattened, gray, then CLAHE (local contrast).
    Clahe,
    /// `enhance_v2`, shown as LCN: lighting flattened by a large median, then band-pass detail
    /// normalised by its robust local contrast (local contrast normalisation); gray.
    EnhanceV2,
}

/// A parameter of a preset: its name in `common.py`, what the panel calls it, and its range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parameter {
    pub name: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Digits after the decimal point; values are rounded to them.
    pub decimals: u32,
}

const fn parameter(name: &'static str, label: &'static str, help: &'static str, default: f64, min: f64, max: f64, decimals: u32) -> Parameter {
    Parameter { name, label, help, default, min, max, decimals }
}

const LIGHT: &str = "Size of the blur that estimates the lighting, as a share of the image's longer side";
const FLATTEN: [Parameter; 1] = [parameter("sigma_frac", "Lighting blur", LIGHT, 0.08, 0.01, 0.5, 2)];
const CLAHE: [Parameter; 3] = [
    parameter("sigma_frac", "Lighting blur", LIGHT, 0.05, 0.01, 0.5, 2),
    parameter("clip", "Clip", "Contrast limit of each tile; higher brings out more", 3.0, 0.5, 10.0, 1),
    parameter("tiles", "Tiles", "Tiles across and down; more makes the contrast more local", 16.0, 2.0, 32.0, 0),
];
const ENHANCE_V2: [Parameter; 5] = [
    parameter("bg_frac", "Background", "Size of the median that estimates the lighting, as a share of the image width", 0.03, 0.005, 0.2, 3),
    parameter("detail_sigma", "Detail blur", "Blur (in pixels) that separates detail from the background", 6.0, 1.0, 30.0, 1),
    parameter("norm_frac", "Contrast area", "Size of the area whose contrast is evened out, as a share of the image width", 0.02, 0.005, 0.2, 3),
    parameter("gain", "Gain", "Strength of the detail", 0.5, 0.0, 2.0, 2),
    parameter("clip", "Clip", "Detail stronger than this many times the local contrast is cut", 3.0, 0.5, 8.0, 1),
];

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Flatten, Preset::Clahe, Preset::EnhanceV2];

    /// Short name, used in keys and on the panel.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Flatten => "flatten",
            Preset::Clahe => "clahe",
            Preset::EnhanceV2 => "lcn",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Preset::Flatten => "Flatten",
            Preset::Clahe => "CLAHE",
            Preset::EnhanceV2 => "LCN",
        }
    }

    pub fn parameters(self) -> &'static [Parameter] {
        match self {
            Preset::Flatten => &FLATTEN,
            Preset::Clahe => &CLAHE,
            Preset::EnhanceV2 => &ENHANCE_V2,
        }
    }

    fn from_name(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.name() == name)
    }
}

/// A preset with a value for each of its parameters, each in range and rounded.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    preset: Preset,
    values: Vec<f64>,
}

impl Filter {
    /// `preset` with its defaults.
    pub fn new(preset: Preset) -> Filter {
        Filter { preset, values: preset.parameters().iter().map(|p| p.default).collect() }
    }

    /// `preset` with `values` (clamped and rounded); missing values take their default.
    pub fn with_values(preset: Preset, values: &[f64]) -> Filter {
        let mut filter = Filter::new(preset);
        for (index, value) in values.iter().enumerate().take(filter.values.len()) {
            filter.set(index, *value);
        }
        filter
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The value of the parameter called `name` in `common.py`.
    pub fn value(&self, name: &str) -> f64 {
        let index = self.preset.parameters().iter().position(|p| p.name == name).unwrap_or_else(|| panic!("{} has no parameter {name}", self.preset.name()));
        self.values[index]
    }

    /// Sets parameter `index`, clamped to its range and rounded to its decimals.
    pub fn set(&mut self, index: usize, value: f64) {
        let Some(parameter) = self.preset.parameters().get(index) else { return };
        let value = if value.is_finite() { value.clamp(parameter.min, parameter.max) } else { parameter.default };
        let scale = 10f64.powi(parameter.decimals as i32);
        // `+ 0.0` turns -0 into 0, so equal values give equal keys.
        self.values[index] = (value * scale).round() / scale + 0.0;
    }

    /// The filter's name in URLs and cache folders, e.g. `clahe_0.05_3.0_16`. Equal filters have
    /// equal keys.
    pub fn key(&self) -> String {
        let mut key = self.preset.name().to_owned();
        for (value, parameter) in self.values.iter().zip(self.preset.parameters()) {
            key.push_str(&format!("_{value:.0$}", parameter.decimals as usize));
        }
        key
    }

    /// The filter a [`key`](Filter::key) names, if it is one: a known preset and one value per
    /// parameter, in range.
    pub fn parse(key: &str) -> Option<Filter> {
        let mut parts = key.split('_');
        let preset = Preset::from_name(parts.next()?)?;
        let values: Vec<f64> = parts.map(|part| part.parse().ok().filter(|v: &f64| v.is_finite())).collect::<Option<_>>()?;
        let parameters = preset.parameters();
        if values.len() != parameters.len() || values.iter().zip(parameters).any(|(v, p)| *v < p.min || *v > p.max) {
            return None;
        }
        Some(Filter::with_values(preset, &values))
    }

    /// Whether the result is gray (one channel) whatever the image.
    pub fn is_gray(&self) -> bool {
        self.preset != Preset::Flatten
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_a_filter_and_its_values_and_read_back() {
        let filter = Filter::new(Preset::Clahe);
        assert_eq!(filter.key(), "clahe_0.05_3.0_16");
        assert_eq!(Filter::parse(&filter.key()), Some(filter));
        let v2 = Filter::with_values(Preset::EnhanceV2, &[0.03, 6.0, 0.02, 0.1 + 0.2, 3.0]);
        assert_eq!(v2.key(), "lcn_0.030_6.0_0.020_0.30_3.0");
        assert_eq!(Filter::parse(&v2.key()).unwrap().value("gain"), 0.3);
        assert_eq!(Filter::new(Preset::Flatten).key(), "flatten_0.08");
    }

    #[test]
    fn values_are_kept_in_range_and_bad_keys_are_refused() {
        let mut filter = Filter::new(Preset::EnhanceV2);
        filter.set(3, 9.0);
        filter.set(1, f64::NAN);
        assert_eq!((filter.value("gain"), filter.value("detail_sigma")), (2.0, 6.0));
        assert_eq!(Filter::parse("lcn_0.030_6.0_0.020_-0.00_3.0").unwrap().key(), "lcn_0.030_6.0_0.020_0.00_3.0");
        for key in ["", "sharpen_1", "flatten", "flatten_0.08_1", "flatten_x", "flatten_0.9", "flatten_inf", "clahe_0.05_3.0"] {
            assert_eq!(Filter::parse(key), None, "{key}");
        }
    }
}
