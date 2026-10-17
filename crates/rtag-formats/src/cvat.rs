//! CVAT "for images 1.1" XML: its labels (with their attribute definitions) and each image's
//! shapes, read into rtag shapes. Matching the images and labels to a project is the importer's
//! business; this only reads the file.
//!
//! Boxes become rectangles, or oriented rectangles when turned; ellipses keep their rotation;
//! masks keep their runs, which CVAT encodes as rtag does (the rows of the mask's box, starting
//! with unset pixels). CVAT angles are degrees clockwise on screen, rtag's radians the same way.
//! Shapes rtag has no type for (cuboids, skeletons) are named in `CvatImage::unsupported`.

use roxmltree::{Document, Node};
use rtag_core::{
    ShapeType,
    annotation::Shape,
    attribute::{AttributeDefinition, AttributeKind},
    ellipse::Ellipse,
    geometry::{Point, Rect},
    mask::Mask,
    oriented::OrientedRect,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CvatExport {
    pub labels: Vec<CvatLabel>,
    pub images: Vec<CvatImage>,
}

/// A label as the export's meta defines it.
#[derive(Debug, Clone, PartialEq)]
pub struct CvatLabel {
    pub name: String,
    pub color: Option<String>,
    /// The shapes its CVAT type allows ("any" allows every one).
    pub shape_types: Vec<ShapeType>,
    pub attributes: Vec<AttributeDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CvatImage {
    /// As CVAT gives it, possibly with folders.
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Lowest `z_order` first (tags before shapes), otherwise in file order.
    pub annotations: Vec<CvatAnnotation>,
    /// The element names of shapes rtag cannot represent, once per shape.
    pub unsupported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CvatAnnotation {
    /// The CVAT label's name.
    pub label: String,
    pub shape: Shape,
    /// Attribute values as written in the file, by name.
    pub attributes: Vec<(String, String)>,
}

pub fn parse_cvat(xml: &str) -> Result<CvatExport, String> {
    let document = Document::parse(xml).map_err(|err| format!("not a valid XML file: {err}"))?;
    let root = document.root_element();
    let version = child_text(root, "version");
    if version.as_deref() != Some("1.1") {
        return Err(format!("expected a CVAT for images 1.1 export, found version {}", version.unwrap_or_else(|| "none".into())));
    }
    // A project export lists labels under meta/project, a task export under meta/task.
    let mut labels: Vec<CvatLabel> = Vec::new();
    let listed = root.descendants().filter(|node| node.has_tag_name("label") && node.parent().is_some_and(|parent| parent.has_tag_name("labels")));
    for node in listed {
        let label = read_label(node)?;
        // A name listed twice is one label, as CVAT's shapes refer to labels by name.
        if !labels.iter().any(|known| known.name == label.name) {
            labels.push(label);
        }
    }
    let images = root.children().filter(|node| node.has_tag_name("image")).map(read_image).collect::<Result<_, _>>()?;
    Ok(CvatExport { labels, images })
}

fn child_text(node: Node, name: &str) -> Option<String> {
    node.children().find(|child| child.has_tag_name(name)).map(|child| child.text().unwrap_or_default().to_owned())
}

fn read_label(node: Node) -> Result<CvatLabel, String> {
    let name = child_text(node, "name").ok_or("a label without a name")?;
    let shape_types = match child_text(node, "type").as_deref() {
        None | Some("any") => ShapeType::ALL.to_vec(),
        Some("rectangle") => vec![ShapeType::Rectangle, ShapeType::OrientedRectangle],
        Some("polygon") => vec![ShapeType::Polygon],
        Some("polyline") => vec![ShapeType::Polyline],
        Some("points") => vec![ShapeType::Points],
        Some("ellipse") => vec![ShapeType::Ellipse],
        Some("mask") => vec![ShapeType::Mask],
        Some("tag") => vec![ShapeType::Tag],
        // Skeletons, cuboids and anything newer: no rtag shape.
        Some(_) => Vec::new(),
    };
    let attributes = node
        .children()
        .filter(|child| child.has_tag_name("attributes"))
        .flat_map(|list| list.children().filter(|child| child.has_tag_name("attribute")))
        .map(|attribute| read_attribute_definition(attribute, &name))
        .collect::<Result<_, _>>()?;
    Ok(CvatLabel { name, color: child_text(node, "color"), shape_types, attributes })
}

fn read_attribute_definition(node: Node, label: &str) -> Result<AttributeDefinition, String> {
    let name = child_text(node, "name").ok_or_else(|| format!("an attribute of \"{label}\" without a name"))?;
    let default = child_text(node, "default_value").unwrap_or_default();
    let values: Vec<String> = child_text(node, "values").unwrap_or_default().lines().map(str::to_owned).collect();
    let number = |text: Option<&String>| text.and_then(|value| value.trim().parse::<f64>().ok());
    let kind = match child_text(node, "input_type").as_deref() {
        Some("checkbox") => AttributeKind::Checkbox { default: default.trim().eq_ignore_ascii_case("true") },
        Some("select" | "radio") => {
            let default = if values.contains(&default) { default } else { values.first().cloned().unwrap_or_default() };
            AttributeKind::Select { options: values, default }
        }
        // CVAT's values for a number are its minimum, maximum and step.
        Some("number") => AttributeKind::Number {
            min: number(values.first()),
            max: number(values.get(1)),
            default: number(Some(&default)).or(number(values.first())).unwrap_or(0.0),
        },
        _ => AttributeKind::Text { default },
    };
    Ok(AttributeDefinition { name, kind })
}

fn read_image(node: Node) -> Result<CvatImage, String> {
    let name = node.attribute("name").ok_or("an image without a name")?.to_owned();
    let size = |key: &str| -> Result<u32, String> {
        node.attribute(key).and_then(|value| value.trim().parse().ok()).ok_or_else(|| format!("image \"{name}\": no valid {key}"))
    };
    let (width, height) = (size("width")?, size("height")?);
    let mut stacked = Vec::new();
    let mut unsupported = Vec::new();
    for element in node.children().filter(Node::is_element) {
        let kind = element.tag_name().name();
        let read = |key: &str| -> Result<f64, String> {
            element
                .attribute(key)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .ok_or_else(|| format!("image \"{name}\": a {kind} without a valid {key}"))
        };
        let points = || -> Result<Vec<Point>, String> {
            let text = element.attribute("points").unwrap_or_default();
            text.split(';')
                .map(|pair| {
                    let (x, y) = pair.split_once(',')?;
                    Some(Point::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
                })
                .collect::<Option<Vec<_>>>()
                .filter(|points| !points.is_empty())
                .ok_or_else(|| format!("image \"{name}\": a {kind} with unreadable points"))
        };
        // CVAT turns boxes and ellipses clockwise by `rotation` degrees about their centre.
        let angle = || element.attribute("rotation").and_then(|value| value.trim().parse::<f64>().ok()).unwrap_or(0.0).to_radians();
        let shape = match kind {
            "box" => {
                let rect = Rect::new(read("xtl")?, read("ytl")?, read("xbr")?, read("ybr")?);
                match angle() {
                    0.0 => Shape::Rectangle(rect),
                    angle => Shape::OrientedRectangle(OrientedRect {
                        centre: rect.min.midpoint(rect.max),
                        width: rect.width(),
                        height: rect.height(),
                        angle,
                    }),
                }
            }
            "polygon" => Shape::Polygon { points: points()? },
            "polyline" => Shape::Polyline { points: points()? },
            "points" => Shape::Points { points: points()? },
            "ellipse" => Shape::Ellipse(Ellipse { centre: Point::new(read("cx")?, read("cy")?), rx: read("rx")?, ry: read("ry")?, angle: angle() }),
            "mask" => {
                let whole = |key: &str| read(key).map(|value| value as u32);
                let mut rle = element
                    .attribute("rle")
                    .unwrap_or_default()
                    .split(',')
                    .map(|run| run.trim().parse::<u32>().ok())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| format!("image \"{name}\": a mask with unreadable runs"))?;
                // A trailing empty run adds nothing.
                while rle.len() > 1 && rle.last() == Some(&0) {
                    rle.pop();
                }
                Shape::Mask(Mask { left: whole("left")?, top: whole("top")?, width: whole("width")?, height: whole("height")?, rle })
            }
            "tag" => Shape::Tag,
            _ => {
                unsupported.push(kind.to_owned());
                continue;
            }
        };
        let label = element.attribute("label").ok_or_else(|| format!("image \"{name}\": a {kind} without a label"))?.to_owned();
        let attributes = element
            .children()
            .filter(|child| child.has_tag_name("attribute"))
            .filter_map(|child| Some((child.attribute("name")?.to_owned(), child.text().unwrap_or_default().to_owned())))
            .collect();
        // Tags lie under every shape.
        let z_order = if kind == "tag" { i64::MIN } else { element.attribute("z_order").and_then(|z| z.trim().parse().ok()).unwrap_or(0) };
        stacked.push((z_order, CvatAnnotation { label, shape, attributes }));
    }
    stacked.sort_by_key(|(z_order, _)| *z_order);
    Ok(CvatImage { name, width, height, annotations: stacked.into_iter().map(|(_, annotation)| annotation).collect(), unsupported })
}
