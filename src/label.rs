//! Foods from a photo of a nutrition label (Swedish or English): the photo is shrunk to a
//! JPEG, Claude reads the per-100 g column, and the result becomes a food for the library.
//! The photo is kept in `label-photos/` so the numbers can be checked against it later.

use crate::foods::Food;
use crate::store::Settings;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Longest side sent to Claude; larger images are scaled down by the API anyway.
const MAX_SIDE: u32 = 1568;

/// File types the PC picker and drag-and-drop accept.
pub const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

const PROMPT: &str = "This is a photo of a food package's nutrition label (Swedish or English; Swedish words: \
Näringsvärde/Näringsinnehåll = nutrition, Energi = energy, Fett = fat, varav mättat fett = of which saturated, \
Kolhydrater = carbohydrates, varav sockerarter = of which sugars, Fiber, Protein, Salt, portion = serving, \
förpackningen innehåller = package contains). Read the values PER 100 g (or per 100 ml for drinks, treating ml as g). \
If only kJ is given, kcal = kJ / 4.184. If only a per-serving column is readable, convert it to per 100 g using the \
serving size. Use the numbers exactly as printed (a comma is a decimal point). For the name, use the product name \
(and brand) from the photo if visible, short, like \"Kvarg naturell 0,2 % (Arla)\"; otherwise a short generic name. \
For the serving, use the label's portion size if there is one, else a typical serving for this product. \
Reply with ONLY this JSON, no other text: {\"name\": text, \"kcal_per_100g\": number, \"protein_per_100g\": number, \
\"carbs_per_100g\": number, \"fat_per_100g\": number, \"serving_g\": number, \"serving_name\": \"one word like portion, \
serving, slice, jar, cup, piece\", \"note\": \"one short sentence if something was unclear or estimated, else empty\"}. \
If the photo isn't a nutrition label or the numbers can't be read, reply {\"error\": \"what's wrong, in one short sentence\"}.";

/// A food read from a label, ready to add to the library.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelFood {
    pub food: Food,
    /// Anything Claude was unsure about.
    pub note: String,
}

/// Decodes a photo (JPEG, PNG or WebP), turns it upright, scales it down and re-encodes it
/// as JPEG.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use image::{ImageDecoder, ImageReader};
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|_| "That isn't a photo Morrowkeep can read (use JPEG, PNG or WebP)".to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| format!("Couldn't read the photo: {e}"))?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img.to_rgb8()).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Sends a prepared JPEG to Claude and reads the food from its reply. Blocking.
pub fn read(settings: &Settings, workdir: &Path, jpeg: &[u8]) -> Result<LabelFood, String> {
    let cli = crate::claude::find_cli(settings).ok_or("Claude Code wasn't found (Settings > Input & voice)")?;
    let reply = crate::claude::ask_image(&cli, &settings.claude_model, PROMPT, jpeg, workdir)?;
    parse(&reply)
}

/// Prepares the photo, reads it and keeps the photo next to the new food's id. Blocking.
pub fn read_photo(settings: &Settings, root: &Path, bytes: &[u8]) -> Result<LabelFood, String> {
    let jpeg = prepare(bytes)?;
    let label = read(settings, root, &jpeg)?;
    let _ = save_photo(root, &label.food.id, &jpeg);
    Ok(label)
}

pub fn photo_path(root: &Path, food_id: &str) -> PathBuf {
    root.join("label-photos").join(format!("{food_id}.jpg"))
}

pub fn save_photo(root: &Path, food_id: &str, jpeg: &[u8]) -> Result<(), String> {
    let path = photo_path(root, food_id);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, jpeg).map_err(|e| e.to_string())
}

/// Claude's reply → a new food (with a fresh id).
pub fn parse(text: &str) -> Result<LabelFood, String> {
    let (a, b) = (text.find('{').ok_or("Claude didn't reply with numbers")?, text.rfind('}').ok_or("Claude didn't reply with numbers")?);
    let v: Value = serde_json::from_str(&text[a..=b]).map_err(|e| e.to_string())?;
    if let Some(e) = v["error"].as_str().filter(|e| !e.trim().is_empty()) {
        return Err(format!("Couldn't read the label: {}", e.trim()));
    }
    let num = |k: &str, max: f64| v[k].as_f64().filter(|n| n.is_finite() && *n >= 0.0 && *n <= max).map(|n| (n * 10.0).round() as f32 / 10.0);
    let kcal = num("kcal_per_100g", 900.0).ok_or("No calories in Claude's reading of the label")?;
    let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Food from label").chars().take(60).collect();
    let serving_name = v["serving_name"].as_str().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty() && s.len() < 20).unwrap_or_else(|| "serving".into());
    Ok(LabelFood {
        food: Food {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            kcal_per_100g: kcal,
            serving_g: num("serving_g", 5000.0).filter(|g| *g >= 1.0).unwrap_or(100.0),
            serving_name,
            protein_per_100g: num("protein_per_100g", 100.0),
            carbs_per_100g: num("carbs_per_100g", 100.0),
            fat_per_100g: num("fat_per_100g", 100.0),
            ..Default::default()
        },
        note: v["note"].as_str().unwrap_or("").trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_replies() {
        let l = parse(r#"{"name":"Kvarg naturell 0,2 % (Arla)","kcal_per_100g":63,"protein_per_100g":11,"carbs_per_100g":3.9,"fat_per_100g":0.2,"serving_g":150,"serving_name":"Portion","note":""}"#).unwrap();
        let f = &l.food;
        assert_eq!((f.name.as_str(), f.kcal_per_100g, f.serving_g, f.serving_name.as_str()), ("Kvarg naturell 0,2 % (Arla)", 63.0, 150.0, "portion"));
        assert_eq!((f.protein_per_100g, f.carbs_per_100g, f.fat_per_100g), (Some(11.0), Some(3.9), Some(0.2)));
        assert!(parse(r#"{"error":"This is a photo of a cat"}"#).unwrap_err().contains("cat"));
        assert!(parse("sorry").is_err());
    }

    #[test]
    fn photos_are_scaled_down_and_turned_into_jpeg() {
        let img = image::RgbImage::from_pixel(3000, 1000, image::Rgb([200, 30, 30]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let jpeg = prepare(&png).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (MAX_SIDE, 523));
        assert!(prepare(b"not an image").is_err());
    }
}
