//! Pure planning for a CVAT import. The HTTP handler supplies the parsed export, the project's
//! labels and images, and the files found in the optional images folder; this module decides what
//! would be created and counts the report without touching a database or reading an upload.

use std::collections::{BTreeMap, BTreeSet};

use oxilabel_core::{
    Label, LabelDefinition, ShapeType,
    annotation::Shape,
    attribute::{AttributeDefinition, AttributeKind, AttributeValue, Attributes},
    import::{CvatImportPlan, CvatImportReport, CvatLabelReport, LabelChoice, SkipCount},
};
use oxilabel_formats::cvat::{CvatAnnotation, CvatExport, CvatLabel};

use crate::images::NewImage;

pub(crate) type ImageKey = (String, u32, u32);
/// Export images the project lacks that the images folder has, by lower-case file name and size:
/// read and ready to add when committing, otherwise only found.
pub(crate) type Found = BTreeMap<ImageKey, Option<NewImage>>;

/// A project image as the planner needs it. `tags` are the labels already present as tags.
#[derive(Debug, Clone)]
pub(crate) struct ProjectImage {
    pub id: i64,
    pub width: u32,
    pub height: u32,
    pub annotations: i64,
    pub tags: BTreeSet<i64>,
}

/// The label a planned annotation gets: one of the project's, or the i-th new label of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LabelRef {
    Existing(i64),
    New(usize),
}

#[derive(Debug)]
pub(crate) struct PlannedAnnotation {
    pub label: LabelRef,
    pub shape: Shape,
    pub attributes: Attributes,
}

pub(crate) enum ImageTarget {
    Existing { id: i64 },
    /// `image` is `None` when previewing: the file was found but not read.
    New { key: ImageKey, image: Option<NewImage> },
}

pub(crate) struct PlannedImage {
    pub target: ImageTarget,
    pub annotations: Vec<PlannedAnnotation>,
}

/// The import as it will be: what the report says, what to create, and what to write per image.
pub(crate) struct CvatPlan {
    pub report: CvatImportReport,
    pub new_labels: Vec<LabelDefinition>,
    pub images: Vec<PlannedImage>,
    pub replace: bool,
}

pub(crate) fn plan(export: &CvatExport, existing: &[Label], images: &BTreeMap<String, Vec<ProjectImage>>, mut found: Found, choices: &CvatImportPlan) -> Result<CvatPlan, String> {
    // Every CVAT label: the listed ones, then any the shapes use without the list naming them.
    let mut cvat_labels: Vec<CvatLabel> = export.labels.clone();
    for annotation in export.images.iter().flat_map(|image| &image.annotations) {
        if !cvat_labels.iter().any(|label| label.name == annotation.label) {
            cvat_labels.push(CvatLabel { name: annotation.label.clone(), color: None, shape_types: ShapeType::ALL.to_vec(), attributes: Vec::new() });
        }
    }

    // Resolve label choices to symbolic references. New labels have no id until commit.
    let mut reports = Vec::new();
    let mut targets: BTreeMap<String, Option<Target>> = BTreeMap::new();
    let mut new_labels = Vec::new();
    for cvat_label in &cvat_labels {
        let raw = definition_of(cvat_label);
        let (mut definition, mut problem) = match raw.clone().validated(existing) {
            Ok(valid) => (valid, None),
            Err(problem) => (raw.clone(), Some(problem)),
        };
        // `validated` checks the project's labels; check labels created earlier in this plan too.
        if problem.is_none() && new_labels.iter().any(|label: &LabelDefinition| label.name == definition.name) {
            problem = Some(format!("a label named \"{}\" already exists", definition.name));
            definition = raw;
        }
        let same_name = existing.iter().find(|label| label.name.eq_ignore_ascii_case(&cvat_label.name));
        let suggestion = match (same_name, &problem) {
            (Some(label), _) => LabelChoice::Map { label: label.id },
            (None, None) => LabelChoice::Create,
            (None, Some(_)) => LabelChoice::Skip,
        };
        let choice = choices.labels.get(&cvat_label.name).copied().unwrap_or(suggestion);
        let target = match choice {
            LabelChoice::Map { label } => Some(Target::Existing(
                existing
                    .iter()
                    .find(|existing| existing.id == label)
                    .cloned()
                    .ok_or_else(|| format!("this project has no label {label} for \"{}\"", cvat_label.name))?,
            )),
            LabelChoice::Create => {
                let index = new_labels.len();
                new_labels.push(definition.clone());
                Some(Target::New(index))
            }
            LabelChoice::Skip => None,
        };
        let count = export.images.iter().flat_map(|image| &image.annotations).filter(|a| a.label == cvat_label.name).count();
        // A problem matters only for a label to be created.
        let problem = problem.filter(|_| choice == LabelChoice::Create);
        reports.push(CvatLabelReport { name: cvat_label.name.clone(), color: cvat_label.color.clone(), annotations: count, choice, problem });
        targets.insert(cvat_label.name.clone(), target);
    }

    let mut report = CvatImportReport {
        labels: reports,
        images: export.images.len(),
        matched: 0,
        added: 0,
        unmatched: Vec::new(),
        ambiguous: Vec::new(),
        size_mismatch: Vec::new(),
        annotated: 0,
        annotations: 0,
        skipped: Vec::new(),
        committed: choices.commit,
    };
    let mut skipped = Vec::new();
    let mut planned_images = Vec::new();
    for cvat_image in &export.images {
        // Same file name (ignoring case), then same size; failing that, an image found in the
        // images folder, added now.
        let key = (file_name(&cvat_image.name).to_lowercase(), cvat_image.width, cvat_image.height);
        let named = images.get(&key.0).map(Vec::as_slice).unwrap_or_default();
        let sized: Vec<&ProjectImage> = named.iter().filter(|image| (image.width, image.height) == (cvat_image.width, cvat_image.height)).collect();
        // Only a name the project has at no matching size is looked for in the folder.
        let found_image = if sized.is_empty() { found.remove(&key) } else { None };
        let (target, existing_tags, had_annotations) = match (named, &sized[..], found_image) {
            (_, [], Some(new)) => {
                report.added += 1;
                (ImageTarget::New { key, image: new }, BTreeSet::new(), 0)
            }
            ([], _, None) => {
                report.unmatched.push(cvat_image.name.clone());
                continue;
            }
            (_, [image], _) => (ImageTarget::Existing { id: image.id }, image.tags.clone(), image.annotations),
            (_, [], None) => {
                report.size_mismatch.push(cvat_image.name.clone());
                continue;
            }
            _ => {
                report.ambiguous.push(cvat_image.name.clone());
                continue;
            }
        };
        report.matched += 1;
        if had_annotations > 0 {
            report.annotated += 1;
        }
        for kind in &cvat_image.unsupported {
            SkipCount::tally(&mut skipped, format!("{kind} shapes are not supported"), 1);
        }
        // Tags already on the image, unless the import replaces them.
        let mut tagged: BTreeSet<LabelRef> = if choices.replace { BTreeSet::new() } else { existing_tags.into_iter().map(LabelRef::Existing).collect() };
        let mut annotations = Vec::new();
        let mut dropped_values = Vec::new();
        let mut refused = Vec::new();
        for cvat_annotation in &cvat_image.annotations {
            let Some(target) = targets.get(&cvat_annotation.label).and_then(Option::as_ref) else {
                SkipCount::tally(&mut skipped, format!("label \"{}\" is skipped", cvat_annotation.label), 1);
                continue;
            };
            let (attributes, dropped) = attributes_of(cvat_annotation, target_attributes(target, &new_labels));
            let check = check_target(target, &new_labels, &cvat_annotation.shape, &attributes, cvat_image.width, cvat_image.height).and_then(|()| {
                if cvat_annotation.shape.is_tag() && !tagged.insert(target.reference()) {
                    Err(format!("the image already has the tag \"{}\"", target_name(target, &new_labels)))
                } else {
                    Ok(())
                }
            });
            match check {
                Ok(()) => {
                    dropped_values.push(dropped);
                    annotations.push(PlannedAnnotation { label: target.reference(), shape: cvat_annotation.shape.clone(), attributes });
                }
                Err(reason) => refused.push(reason),
            }
        }
        report.annotations += annotations.len();
        // As in the store, a refused shape does not also count its dropped attribute values.
        for dropped in dropped_values {
            for reason in dropped {
                SkipCount::tally(&mut skipped, reason.to_owned(), 1);
            }
        }
        for reason in refused {
            SkipCount::tally(&mut skipped, reason, 1);
        }
        planned_images.push(PlannedImage { target, annotations });
    }
    report.skipped = skipped;
    Ok(CvatPlan { report, new_labels, images: planned_images, replace: choices.replace })
}

/// A resolved choice: an existing label (kept so its definition can check annotations) or a new
/// label by its index in `CvatPlan::new_labels`.
enum Target {
    Existing(Label),
    New(usize),
}

impl Target {
    fn reference(&self) -> LabelRef {
        match self {
            Target::Existing(label) => LabelRef::Existing(label.id),
            Target::New(index) => LabelRef::New(*index),
        }
    }
}

fn target_name<'a>(target: &'a Target, new_labels: &'a [LabelDefinition]) -> &'a str {
    match target {
        Target::Existing(label) => &label.name,
        Target::New(index) => &new_labels[*index].name,
    }
}

fn target_attributes<'a>(target: &'a Target, new_labels: &'a [LabelDefinition]) -> &'a [AttributeDefinition] {
    match target {
        Target::Existing(label) => &label.attributes,
        Target::New(index) => &new_labels[*index].attributes,
    }
}

fn check_target(target: &Target, new_labels: &[LabelDefinition], shape: &Shape, attributes: &Attributes, width: u32, height: u32) -> Result<(), String> {
    match target {
        Target::Existing(label) => label.check_annotation(shape, attributes, width, height),
        Target::New(index) => new_labels[*index].check_annotation(shape, attributes, width, height),
    }
}

/// The OxiLabel label a CVAT label becomes: its name, color, allowed shapes and attributes.
fn definition_of(label: &CvatLabel) -> LabelDefinition {
    LabelDefinition {
        name: label.name.clone(),
        color: label.color.clone().unwrap_or_else(|| "#7c8a99".to_owned()),
        hotkey: None,
        shape_types: label.shape_types.clone(),
        ignore: false,
        attributes: label.attributes.clone(),
        footprint: Default::default(),
        show_footprint: true,
        constraints: Default::default(),
    }
}

/// The CVAT attribute values that the label has an attribute for and that fit it, and why each of
/// the others is left out.
fn attributes_of(annotation: &CvatAnnotation, definitions: &[AttributeDefinition]) -> (Attributes, Vec<&'static str>) {
    let (mut values, mut dropped) = (Attributes::new(), Vec::new());
    for (name, text) in &annotation.attributes {
        let Some(definition) = definitions.iter().find(|definition| &definition.name == name) else {
            dropped.push("attribute values with no matching attribute");
            continue;
        };
        let value = match &definition.kind {
            AttributeKind::Checkbox { .. } => match text.trim().to_ascii_lowercase().as_str() {
                "true" => Some(AttributeValue::Bool(true)),
                "false" => Some(AttributeValue::Bool(false)),
                _ => None,
            },
            AttributeKind::Number { .. } => text.trim().parse().ok().map(AttributeValue::Number),
            AttributeKind::Select { .. } => Some(AttributeValue::Text(text.trim().to_owned())),
            AttributeKind::Text { .. } => Some(AttributeValue::Text(text.clone())),
        };
        match value.filter(|value| definition.check(value).is_ok()) {
            Some(value) => {
                values.insert(name.clone(), value);
            }
            None => dropped.push("attribute values that do not fit their attribute"),
        }
    }
    (values, dropped)
}

/// The file name of a path with `/` or `\` separators.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxilabel_formats::cvat::parse_cvat;

    fn definition(name: &str, shape_types: Vec<ShapeType>) -> LabelDefinition {
        LabelDefinition { name: name.into(), color: "#ff0000".into(), hotkey: None, shape_types, ignore: false, attributes: Vec::new(), footprint: Default::default(), show_footprint: true, constraints: Default::default() }
    }

    fn label(id: i64, name: &str, shape_types: Vec<ShapeType>) -> Label {
        Label::new(id, definition(name, shape_types))
    }

    fn image(id: i64, width: u32, height: u32) -> ProjectImage {
        ProjectImage { id, width, height, annotations: 0, tags: BTreeSet::new() }
    }

    fn export(body: &str) -> CvatExport {
        parse_cvat(&format!("<annotations><version>1.1</version>{body}</annotations>")).unwrap()
    }

    fn box_image(name: &str, width: u32, label: &str) -> String {
        format!(r#"<image id="0" name="{name}" width="{width}" height="20"><box label="{label}" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/></image>"#)
    }

    fn skipped(report: &CvatImportReport, reason: &str) -> usize {
        report.skipped.iter().find(|skip| skip.reason == reason).map_or(0, |skip| skip.count)
    }

    #[test]
    fn labels_are_mapped_created_or_skipped_and_annotations_reference_the_plan() {
        let export = export(r#"
          <meta><task><labels>
            <label><name>crack</name><color>#e6194b</color><type>any</type><attributes></attributes></label>
            <label><name>dust</name><color>#00ff00</color><type>any</type><attributes></attributes></label>
            <label><name>new</name><color>#0000ff</color><type>any</type><attributes></attributes></label>
          </labels></task></meta>
          <image id="0" name="a.png" width="30" height="20">
            <box label="crack" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/>
            <box label="dust" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/>
            <box label="new" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0"/>
          </image>"#);
        let existing = vec![label(7, "Crack", ShapeType::ALL.to_vec())];
        let images = BTreeMap::from([("a.png".into(), vec![image(11, 30, 20)])]);
        let mut choices = CvatImportPlan::default();
        choices.labels.insert("dust".into(), LabelChoice::Skip);
        let planned = plan(&export, &existing, &images, Found::new(), &choices).unwrap();

        assert_eq!(planned.report.labels.iter().map(|l| l.choice).collect::<Vec<_>>(), [
            LabelChoice::Map { label: 7 },
            LabelChoice::Skip,
            LabelChoice::Create,
        ]);
        assert_eq!(planned.new_labels.len(), 1);
        assert_eq!(planned.report.annotations, 2);
        assert_eq!(skipped(&planned.report, "label \"dust\" is skipped"), 1);
        let refs: Vec<_> = planned.images[0].annotations.iter().map(|a| a.label).collect();
        assert_eq!(refs, [LabelRef::Existing(7), LabelRef::New(0)]);
        assert!(planned.new_labels.iter().all(|label| label.name == "new"));
    }

    #[test]
    fn images_match_by_name_and_size_and_use_only_the_chosen_folder_results() {
        let export = export(&[
            box_image("folder/one.png", 30, "x"), box_image("two.png", 30, "x"),
            box_image("three.png", 40, "x"), box_image("four.png", 30, "x"),
            box_image("five.png", 30, "x"),
        ].concat());
        let images = BTreeMap::from([
            ("one.png".into(), vec![image(1, 30, 20)]),
            ("two.png".into(), vec![image(2, 30, 20), image(3, 30, 20)]),
            ("three.png".into(), vec![image(4, 30, 20)]),
        ]);
        let found = BTreeMap::from([(("four.png".to_owned(), 30, 20), None)]);
        let planned = plan(&export, &[], &images, found, &CvatImportPlan::default()).unwrap();

        assert_eq!((planned.report.matched, planned.report.added), (2, 1));
        assert_eq!(planned.report.ambiguous, ["two.png"]);
        assert_eq!(planned.report.size_mismatch, ["three.png"]);
        assert_eq!(planned.report.unmatched, ["five.png"]);
        assert_eq!(planned.report.annotations, 2);
        assert!(matches!(planned.images[0].target, ImageTarget::Existing { id: 1 }));
        assert!(matches!(planned.images[1].target, ImageTarget::New { ref key, .. } if key.0 == "four.png"));
    }

    #[test]
    fn attributes_tags_and_refused_shapes_are_counted_once() {
        let export = export(r#"
          <meta><task><labels>
            <label><name>defect</name><color>#ff0000</color><type>any</type><attributes>
              <attribute><name>severity</name><input_type>select</input_type><default_value>low</default_value><values>low
high</values></attribute>
              <attribute><name>length</name><input_type>number</input_type><default_value>2</default_value><values>0
10
1</values></attribute>
            </attributes></label>
            <label><name>good</name><color>#00ff00</color><type>tag</type><attributes></attributes></label>
          </labels></task></meta>
          <image id="0" name="a.png" width="30" height="20">
            <tag label="good"/><tag label="good"/>
            <box label="defect" xtl="1" ytl="1" xbr="5" ybr="5" z_order="0">
              <attribute name="severity"> high
</attribute><attribute name="length">11</attribute><attribute name="colour">red</attribute>
            </box>
            <box label="defect" xtl="100" ytl="1" xbr="105" ybr="5" z_order="0"><attribute name="colour">red</attribute></box>
          </image>"#);
        let images = BTreeMap::from([("a.png".into(), vec![image(1, 30, 20)])]);
        let planned = plan(&export, &[], &images, Found::new(), &CvatImportPlan::default()).unwrap();

        assert_eq!(planned.report.annotations, 2);
        assert_eq!(skipped(&planned.report, "the image already has the tag \"good\""), 1);
        assert_eq!(skipped(&planned.report, "attribute values with no matching attribute"), 1);
        assert_eq!(skipped(&planned.report, "attribute values that do not fit their attribute"), 1);
        assert_eq!(planned.report.skipped.iter().map(|s| s.count).sum::<usize>(), 4);
        let attributes = &planned.images[0].annotations[1].attributes;
        assert_eq!(attributes.get("severity"), Some(&AttributeValue::Text("high".into())));
        assert!(!attributes.contains_key("length"));
    }

    #[test]
    fn append_skips_tags_already_on_an_image_but_replace_does_not() {
        let export = export(r#"<image id="0" name="a.png" width="30" height="20"><tag label="good"/></image>"#);
        let existing = vec![label(4, "good", vec![ShapeType::Tag])];
        let mut with_tag = image(1, 30, 20);
        with_tag.annotations = 1;
        with_tag.tags.insert(4);
        let images = BTreeMap::from([("a.png".into(), vec![with_tag])]);
        let mut choices = CvatImportPlan::default();
        choices.labels.insert("good".into(), LabelChoice::Map { label: 4 });
        let append = plan(&export, &existing, &images, Found::new(), &choices).unwrap();
        assert_eq!(append.report.annotations, 0);
        assert_eq!(skipped(&append.report, "the image already has the tag \"good\""), 1);
        choices.replace = true;
        let replace = plan(&export, &existing, &images, Found::new(), &choices).unwrap();
        assert_eq!(replace.report.annotations, 1);
    }

    #[test]
    fn invalid_mappings_and_duplicate_new_label_names_are_reported() {
        let export = export(r#"
          <meta><task><labels>
            <label><name>chip</name><color>#00ff00</color><type>any</type><attributes></attributes></label>
            <label><name>chip </name><color>#00ff00</color><type>any</type><attributes></attributes></label>
          </labels></task></meta>"#);
        let existing = vec![label(1, "other", ShapeType::ALL.to_vec())];
        let mut choices = CvatImportPlan::default();
        choices.labels.insert("chip".into(), LabelChoice::Create);
        choices.labels.insert("chip ".into(), LabelChoice::Create);
        let planned = plan(&export, &existing, &BTreeMap::new(), Found::new(), &choices).unwrap();
        assert_eq!(planned.report.labels[0].problem, None);
        assert_eq!(planned.report.labels[1].problem.as_deref(), Some("a label named \"chip\" already exists"));
        choices.labels.insert("chip".into(), LabelChoice::Map { label: 99 });
        assert!(plan(&export, &existing, &BTreeMap::new(), Found::new(), &choices).is_err());
    }
}