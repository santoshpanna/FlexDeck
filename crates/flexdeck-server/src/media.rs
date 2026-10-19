//! Pictures and icon packs the person adds.
//!
//! # Pictures
//!
//! An uploaded picture is decoded — refusing one larger than 4096 pixels a
//! side before a byte of it is held — scaled to at most 1024, and written
//! again, so what is kept is a plain image this agent made, never the file as
//! sent. It is named by the hash of what was kept (`img:<sha256>`) and lives on
//! disk, so a key's picture outlasts a restart; the same `GET /api/images/{sha}`
//! serves it as serves album art.
//!
//! # Icon packs
//!
//! A zip of icons: this deck's own (`pack.json`), a pack in the common layout
//! other deck apps use (`manifest.json` + `icons.json` + PNGs), or simply PNG
//! and SVG files. PNGs
//! are kept as pictures. An SVG is turned into path data here, at install —
//! its paths, circles, rectangles and lines — so no renderer ever parses SVG.
//! An icon is then `pack:<pack>/<name>`.

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The largest picture accepted, before decoding.
pub const MAX_UPLOAD: usize = 8 * 1024 * 1024;
/// The largest side a picture may have, decoded.
pub const MAX_SIDE: u32 = 4096;
/// The largest side a kept picture has.
pub const KEPT_SIDE: u32 = 1024;
/// A pack icon's largest side.
pub const ICON_SIDE: u32 = 256;
pub const MAX_PACK: usize = 32 * 1024 * 1024;
pub const MAX_ICONS: usize = 5_000;
const MAX_CATEGORY: usize = 40;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A picture as kept: its name and type.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    pub sha256: String,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// Decode `bytes`, refusing anything too large, scale it to fit `side`, and
/// write it again: PNG when it has transparency, JPEG when not.
pub fn normalise(bytes: &[u8], side: u32) -> Result<(Vec<u8>, &'static str, u32, u32), String> {
    if bytes.len() > MAX_UPLOAD {
        return Err(format!("the picture is larger than {} MB", MAX_UPLOAD / 1024 / 1024));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    match reader.format() {
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP) => {}
        _ => return Err("only PNG, JPEG and WebP pictures can be used".into()),
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("the picture could not be read: {error}"))?;
    let scaled = if decoded.width() > side || decoded.height() > side {
        decoded.resize(side, side, image::imageops::FilterType::Triangle)
    } else {
        decoded
    };
    let (width, height) = (scaled.width(), scaled.height());
    let mut out = Vec::new();
    let transparent = scaled.color().has_alpha();
    let mime = if transparent {
        scaled
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        "image/png"
    } else {
        let rgb = scaled.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), 86)
            .encode_image(&rgb)
            .map_err(|error| error.to_string())?;
        "image/jpeg"
    };
    Ok((out, mime, width, height))
}

/// Pictures kept on disk, by hash.
#[derive(Debug)]
pub struct Pictures {
    dir: PathBuf,
}

impl Pictures {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, sha: &str, mime: &str) -> PathBuf {
        self.dir.join(format!("{sha}.{}", if mime == "image/png" { "png" } else { "jpg" }))
    }

    /// Keep a picture, made safe and small. Returns its name.
    pub fn put(&self, bytes: &[u8], side: u32) -> Result<Picture, String> {
        let (kept, mime, width, height) = normalise(bytes, side)?;
        let sha256 = hex(&Sha256::digest(&kept));
        let path = self.path(&sha256, mime);
        if !path.exists() {
            std::fs::write(&path, &kept).map_err(|error| format!("the picture could not be kept: {error}"))?;
        }
        Ok(Picture { sha256, mime, width, height, bytes: kept.len() })
    }

    pub fn get(&self, sha: &str) -> Option<(&'static str, Vec<u8>)> {
        if !crate::images::is_sha256(sha) {
            return None;
        }
        for mime in ["image/png", "image/jpeg"] {
            if let Ok(bytes) = std::fs::read(self.path(sha, mime)) {
                return Some((mime, bytes));
            }
        }
        None
    }

    /// Every kept picture, newest first.
    pub fn list(&self) -> Vec<Picture> {
        let mut found: Vec<(std::time::SystemTime, Picture)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let (sha, ext) = name.split_once('.')?;
                if !crate::images::is_sha256(sha) {
                    return None;
                }
                let mime = match ext {
                    "png" => "image/png",
                    "jpg" => "image/jpeg",
                    _ => return None,
                };
                let meta = entry.metadata().ok()?;
                let (width, height) = image::image_dimensions(entry.path()).unwrap_or((0, 0));
                Some((
                    meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    Picture { sha256: sha.to_string(), mime, width, height, bytes: meta.len() as usize },
                ))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.into_iter().map(|(_, picture)| picture).collect()
    }

    pub fn remove(&self, sha: &str) -> bool {
        crate::images::is_sha256(sha)
            && ["image/png", "image/jpeg"].iter().any(|mime| std::fs::remove_file(self.path(sha, mime)).is_ok())
    }
}

// ---- Icon packs ----------------------------------------------------------------

/// One icon: a kept picture, or paths drawn on a `view` box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackIcon {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// A picture: `img:<sha256>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// SVG path data, each drawn on a `view` box.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<[f64; 4]>,
    /// Filled shapes; otherwise stroked lines.
    #[serde(default)]
    pub fill: bool,
    /// The stroke width on the view box, for line icons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    pub icons: Vec<PackIcon>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub author: String,
    pub count: usize,
}

/// What an install did: the pack, and what was left out and why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub pack: PackSummary,
    pub skipped: Vec<String>,
}

/// Packs kept on disk, one JSON file each.
#[derive(Debug)]
pub struct Packs {
    dir: PathBuf,
}

pub(crate) fn pack_id(name: &str) -> String {
    let id: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let id = id.trim_start_matches(|c: char| c.is_ascii_digit()).chars().take(40).collect::<String>();
    if id.is_empty() { "pack".into() } else { id }
}

fn tags_of(icon: &serde_json::Value) -> Vec<String> {
    icon["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect()
}

fn category_of(icon: &serde_json::Value) -> Option<String> {
    let category: String = icon["category"].as_str()?.trim().chars().take(MAX_CATEGORY).collect();
    (!category.is_empty()).then_some(category)
}

fn icon_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
    stem.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

impl Packs {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn list(&self) -> Vec<PackSummary> {
        let mut packs: Vec<PackSummary> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let pack: Pack = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
                Some(PackSummary { id: pack.id, name: pack.name, author: pack.author, count: pack.icons.len() })
            })
            .collect();
        packs.sort_by(|a, b| a.name.cmp(&b.name));
        packs
    }

    pub fn get(&self, id: &str) -> Option<Pack> {
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        serde_json::from_slice(&std::fs::read(self.path(id)).ok()?).ok()
    }

    pub fn remove(&self, id: &str) -> bool {
        id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && std::fs::remove_file(self.path(id)).is_ok()
    }

    /// Install a zip of icons. PNGs become pictures in `pictures`.
    pub fn install(&self, zip: &[u8], pictures: &Pictures) -> Result<Installed, String> {
        if zip.len() > MAX_PACK {
            return Err(format!("the pack is larger than {} MB", MAX_PACK / 1024 / 1024));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|error| format!("not a zip: {error}"))?;
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut total = 0usize;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let Some(name) = entry.enclosed_name().map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
            let mut bytes = Vec::new();
            (&mut entry).take(MAX_UPLOAD as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            total += bytes.len();
            if total > MAX_PACK * 4 {
                return Err("the pack unpacks to too much".into());
            }
            files.insert(name, bytes);
        }

        // What the pack calls itself, and which files are its icons.
        let find = |suffix: &str| files.keys().find(|name| name.ends_with(suffix)).cloned();
        let mut name = String::new();
        let mut author = String::new();
        let mut license = String::new();
        let mut listed: Vec<(String, String, Vec<String>, Option<String>)> = Vec::new(); // (file, name, tags, category)
        if let Some(own) = find("pack.json") {
            let doc: serde_json::Value = serde_json::from_slice(&files[&own]).map_err(|e| format!("pack.json: {e}"))?;
            name = doc["name"].as_str().unwrap_or_default().to_string();
            author = doc["author"].as_str().unwrap_or_default().to_string();
            license = doc["license"].as_str().unwrap_or_default().to_string();
            let base = own.trim_end_matches("pack.json").to_string();
            for icon in doc["icons"].as_array().into_iter().flatten() {
                let file = format!("{base}{}", icon["file"].as_str().unwrap_or_default());
                let icon_name = icon["name"].as_str().map(str::to_string).unwrap_or_else(|| icon_name(&file));
                listed.push((file, icon_name, tags_of(icon), category_of(icon)));
            }
        } else if let Some(manifest) = find("manifest.json") {
            // The common layout: `manifest.json`, `icons.json` and `icons/`.
            let doc: serde_json::Value = serde_json::from_slice(&files[&manifest]).map_err(|e| format!("manifest.json: {e}"))?;
            name = doc["Name"].as_str().or(doc["name"].as_str()).unwrap_or_default().to_string();
            author = doc["Author"].as_str().or(doc["author"].as_str()).unwrap_or_default().to_string();
            let base = manifest.trim_end_matches("manifest.json").to_string();
            if let Some(list) = files.get(&format!("{base}icons.json")) {
                let icons: serde_json::Value = serde_json::from_slice(list).map_err(|e| format!("icons.json: {e}"))?;
                for icon in icons.as_array().into_iter().flatten() {
                    let path = icon["path"].as_str().unwrap_or_default();
                    let file = format!("{base}icons/{path}");
                    let file = if files.contains_key(&file) { file } else { format!("{base}{path}") };
                    listed.push((file, icon["name"].as_str().map(str::to_string).unwrap_or_else(|| icon_name(path)), tags_of(icon), category_of(icon)));
                }
            }
            if let Some(text) = find("license.txt").and_then(|f| String::from_utf8(files[&f].clone()).ok()) {
                license = text.lines().next().unwrap_or_default().chars().take(200).collect();
            }
        }
        if listed.is_empty() {
            // Just icons: every PNG and SVG, named by its file.
            for file in files.keys().filter(|f| f.ends_with(".png") || f.ends_with(".svg")) {
                if !file.contains("preview") {
                    listed.push((file.clone(), icon_name(file), Vec::new(), None));
                }
            }
        }
        if listed.is_empty() {
            return Err("there are no icons in it: PNG or SVG files, or a pack.json listing them".into());
        }
        if listed.len() > MAX_ICONS {
            return Err(format!("at most {MAX_ICONS} icons in a pack"));
        }
        if name.trim().is_empty() {
            name = "Icon pack".into();
        }

        let mut icons = Vec::new();
        let mut skipped = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (file, icon, tags, category) in listed {
            let Some(bytes) = files.get(&file) else {
                skipped.push(format!("{icon}: {file} is not in the pack"));
                continue;
            };
            let mut unique = icon.clone();
            for n in 2.. {
                if seen.insert(unique.clone()) {
                    break;
                }
                unique = format!("{icon}-{n}");
            }
            let made = if file.ends_with(".svg") {
                svg_icon(&String::from_utf8_lossy(bytes)).map(|(paths, view, fill, stroke)| PackIcon {
                    name: unique.clone(),
                    tags,
                    category,
                    image: None,
                    paths,
                    view: Some(view),
                    fill,
                    stroke,
                })
            } else {
                pictures.put(bytes, ICON_SIDE).map(|picture| PackIcon {
                    name: unique.clone(),
                    tags,
                    category,
                    image: Some(format!("img:{}", picture.sha256)),
                    paths: Vec::new(),
                    view: None,
                    fill: false,
                    stroke: None,
                })
            };
            match made {
                Ok(icon) => icons.push(icon),
                Err(why) => skipped.push(format!("{unique}: {why}")),
            }
        }
        if icons.is_empty() {
            return Err(format!("none of its icons could be used: {}", skipped.first().cloned().unwrap_or_default()));
        }
        let mut id = pack_id(&name);
        for n in 2.. {
            if !self.path(&id).exists() {
                break;
            }
            id = format!("{}-{n}", pack_id(&name));
        }
        let pack = Pack { id: id.clone(), name, author, license, icons };
        std::fs::write(self.path(&id), serde_json::to_vec(&pack).map_err(|e| e.to_string())?)
            .map_err(|error| format!("the pack could not be kept: {error}"))?;
        Ok(Installed {
            pack: PackSummary { id, name: pack.name, author: pack.author, count: pack.icons.len() },
            skipped,
        })
    }
}

// ---- SVG, as paths ---------------------------------------------------------------

/// The attributes of each element in `svg`, in order, skipping what is
/// inside `<defs>`, `<clipPath>`, `<mask>` and `<style>`.
fn elements(svg: &str) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out = Vec::new();
    let mut hidden = 0usize;
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            if matches!(closing.trim(), "defs" | "clipPath" | "mask" | "style") {
                hidden = hidden.saturating_sub(1);
            }
            continue;
        }
        let self_closing = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        if matches!(name, "defs" | "clipPath" | "mask" | "style") {
            if !self_closing {
                hidden += 1;
            }
            continue;
        }
        if hidden > 0 {
            continue;
        }
        let mut map = BTreeMap::new();
        let mut text = attrs;
        while let Some(eq) = text.find('=') {
            let key = text[..eq].trim().to_string();
            let after = text[eq + 1..].trim_start();
            let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(close) = after[1..].find(quote) else { break };
            map.insert(key, after[1..1 + close].to_string());
            text = &after[close + 2..];
        }
        out.push((name.to_string(), map));
    }
    out
}

fn number(attrs: &BTreeMap<String, String>, key: &str) -> f64 {
    attrs.get(key).and_then(|v| v.trim().trim_end_matches("px").parse().ok()).unwrap_or(0.0)
}

/// An SVG icon as (paths, view box, filled, stroke width). Refuses one that
/// moves its shapes about (`transform`), which paths alone cannot carry.
pub fn svg_icon(svg: &str) -> Result<(Vec<String>, [f64; 4], bool, Option<f64>), String> {
    let all = elements(svg);
    let root = all.iter().find(|(name, _)| name == "svg").ok_or("it is not an SVG")?;
    let view = root
        .1
        .get("viewBox")
        .map(|v| v.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse::<f64>().ok()).collect::<Vec<_>>())
        .filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0)
        .map(|v| [v[0], v[1], v[2], v[3]])
        .or_else(|| {
            let (w, h) = (number(&root.1, "width"), number(&root.1, "height"));
            (w > 0.0 && h > 0.0).then_some([0.0, 0.0, w, h])
        })
        .unwrap_or([0.0, 0.0, 24.0, 24.0]);
    let fill = root.1.get("fill").map(|f| f.trim() != "none").unwrap_or(true);
    let stroke = (!fill).then(|| root.1.get("stroke-width").and_then(|w| w.parse().ok()).unwrap_or(2.0));

    let mut paths = Vec::new();
    for (name, attrs) in &all {
        if attrs.contains_key("transform") && name != "svg" {
            return Err("it moves its shapes with transform, which is not supported".into());
        }
        let n = |key: &str| number(attrs, key);
        let d = match name.as_str() {
            "path" => attrs.get("d").cloned().unwrap_or_default(),
            "circle" => {
                let (cx, cy, r) = (n("cx"), n("cy"), n("r"));
                format!("M{} {}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0", cx - r, cy, 2.0 * r, -2.0 * r)
            }
            "ellipse" => {
                let (cx, cy, rx, ry) = (n("cx"), n("cy"), n("rx"), n("ry"));
                format!("M{} {}a{rx} {ry} 0 1 0 {} 0a{rx} {ry} 0 1 0 {} 0", cx - rx, cy, 2.0 * rx, -2.0 * rx)
            }
            "rect" => {
                let (x, y, w, h) = (n("x"), n("y"), n("width"), n("height"));
                let r = n("rx").max(n("ry")).min(w / 2.0).min(h / 2.0);
                if r > 0.0 {
                    format!(
                        "M{} {y}h{}a{r} {r} 0 0 1 {r} {r}v{}a{r} {r} 0 0 1 -{r} {r}h-{}a{r} {r} 0 0 1 -{r} -{r}v-{}a{r} {r} 0 0 1 {r} -{r}z",
                        x + r, w - 2.0 * r, h - 2.0 * r, w - 2.0 * r, h - 2.0 * r
                    )
                } else {
                    format!("M{x} {y}h{w}v{h}h-{w}z")
                }
            }
            "line" => format!("M{} {}L{} {}", n("x1"), n("y1"), n("x2"), n("y2")),
            "polyline" | "polygon" => {
                let points: Vec<&str> = attrs.get("points").map(|p| p.split([' ', ',']).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
                let pairs: Vec<String> = points.chunks(2).filter(|c| c.len() == 2).map(|c| format!("{} {}", c[0], c[1])).collect();
                if pairs.is_empty() {
                    continue;
                }
                format!("M{}{}", pairs.join("L"), if name == "polygon" { "z" } else { "" })
            }
            _ => continue,
        };
        if !d.trim().is_empty() {
            paths.push(d);
        }
    }
    if paths.is_empty() {
        return Err("it has nothing to draw".into());
    }
    Ok((paths, view, fill, stroke))
}

/// Where pictures and packs live: `<data>/media/pictures`, `…/packs`.
#[derive(Debug)]
pub struct Media {
    pub pictures: Pictures,
    pub packs: Packs,
}

impl Media {
    pub fn new(dir: &Path) -> std::io::Result<Self> {
        Ok(Self { pictures: Pictures::new(dir.join("pictures"))?, packs: Packs::new(dir.join("packs"))? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if alpha {
            image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 128]))
                .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .unwrap();
        } else {
            image::RgbImage::from_pixel(width, height, image::Rgb([0, 128, 255]))
                .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .unwrap();
        }
        out
    }

    #[test]
    fn a_picture_is_remade_small_and_kept_by_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let pictures = Pictures::new(dir.path().to_path_buf()).unwrap();
        let photo = pictures.put(&png(2000, 1000, false), KEPT_SIDE).unwrap();
        assert_eq!((photo.width, photo.height, photo.mime), (1024, 512, "image/jpeg"), "no alpha: JPEG, scaled");
        let icon = pictures.put(&png(64, 64, true), KEPT_SIDE).unwrap();
        assert_eq!((icon.width, icon.mime), (64, "image/png"), "transparency kept, not enlarged");
        assert_eq!(pictures.get(&photo.sha256).unwrap().0, "image/jpeg");
        assert_eq!(pictures.list().len(), 2);
        assert!(pictures.remove(&icon.sha256));
        assert!(pictures.get(&icon.sha256).is_none());
        assert!(pictures.put(b"<svg/>", KEPT_SIDE).is_err(), "not a picture");
        assert!(pictures.get("../../etc/passwd").is_none());
    }

    #[test]
    fn a_picture_too_large_to_hold_is_refused_unread() {
        // A header claiming 20000 × 20000: refused by its size, not decoded.
        let header = {
            let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
            bytes.extend_from_slice(&20_000u32.to_be_bytes());
            bytes.extend_from_slice(&20_000u32.to_be_bytes());
            bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
            bytes
        };
        assert!(normalise(&header, KEPT_SIDE).is_err());
    }

    #[test]
    fn svg_shapes_become_paths_and_a_transform_is_refused() {
        let lucide = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/><rect x="2" y="2" width="4" height="4" rx="1"/><line x1="0" y1="0" x2="1" y2="1"/><polyline points="1 2 3 4 5 6"/><defs><path d="M0 0" /></defs></svg>"#;
        let (paths, view, fill, stroke) = svg_icon(lucide).unwrap();
        assert_eq!(view, [0.0, 0.0, 24.0, 24.0]);
        assert!(!fill && stroke == Some(2.0), "a line icon");
        assert_eq!(paths.len(), 5, "defs left out: {paths:?}");
        assert!(paths[0].starts_with("M2 12a10 10"));
        assert_eq!(paths[4], "M1 2L3 4L5 6");

        let material = r#"<svg viewBox="0 -960 960 960"><path d="M480-80q-83 0-156-31z"/></svg>"#;
        let (_, view, fill, _) = svg_icon(material).unwrap();
        assert_eq!(view, [0.0, -960.0, 960.0, 960.0]);
        assert!(fill);
        assert!(svg_icon(r#"<svg viewBox="0 0 1 1"><g transform="scale(2)"><path d="M0 0"/></g></svg>"#).is_err());
        assert!(svg_icon("<svg></svg>").is_err());
    }

    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            for (name, bytes) in files {
                writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn a_manifest_and_icons_json_pack_installs_with_its_names_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let media = Media::new(dir.path()).unwrap();
        let red = png(144, 144, true);
        let pack = zip(&[
            ("retro-keys/manifest.json", br#"{"Name": "Retro Keys", "Author": "Someone", "Version": "1.0"}"#),
            ("retro-keys/icons.json", br#"[{"path": "play.png", "name": "Play", "tags": ["media"], "category": " Media "}, {"path": "gone.png", "name": "Gone"}]"#),
            ("retro-keys/icons/play.png", &red),
        ]);
        let installed = media.packs.install(&pack, &media.pictures).unwrap();
        assert_eq!(installed.pack.id, "retro-keys");
        assert_eq!(installed.pack.count, 1);
        assert_eq!(installed.skipped.len(), 1, "the missing file is reported, not fatal");
        let kept = media.packs.get("retro-keys").unwrap();
        assert_eq!(kept.icons[0].name, "Play");
        assert_eq!(kept.icons[0].tags, ["media"]);
        assert_eq!(kept.icons[0].category.as_deref(), Some("Media"));
        assert!(kept.icons[0].image.as_deref().unwrap().starts_with("img:"));

        let loose = zip(&[("clock.svg", br#"<svg viewBox="0 0 24 24" fill="none" stroke-width="1.5"><circle cx="12" cy="12" r="9"/></svg>"#), ("note.png", &red)]);
        let second = media.packs.install(&loose, &media.pictures).unwrap();
        assert_eq!(second.pack.count, 2);
        assert_eq!(media.packs.list().len(), 2);
        assert!(media.packs.install(b"not a zip", &media.pictures).is_err());
        assert!(media.packs.remove(&second.pack.id));
    }
}
