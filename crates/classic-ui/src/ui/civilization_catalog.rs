//! Artwork identities are separate from playable civilization definitions.
use super::Asset;
use std::{collections::VecDeque, path::PathBuf, sync::OnceLock};

#[derive(Clone, Debug)]
pub(super) struct Card {
    pub key: String,
    pub name: String,
    pub leader: String,
}

pub(super) fn cards() -> Vec<Card> {
    include_str!("../../../../assets/visual/generated/civilizations/artwork-catalog.tsv")
        .lines().skip(1).filter_map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            (fields.len() == 3).then(|| Card {
                key: fields[0].to_owned(), name: fields[1].to_owned(), leader: fields[2].to_owned(),
            })
        }).collect()
}

struct Region {
    key: String,
    role: String,
    path: String,
    rect: [u32; 4],
}
fn regions() -> &'static [Region] {
    static INDEX: OnceLock<Vec<Region>> = OnceLock::new();
    INDEX.get_or_init(|| {
        include_str!("../../../../assets/visual/generated/civilizations/artwork-index.tsv")
            .lines().skip(1).filter_map(|line| {
                let f: Vec<_> = line.split('\t').collect();
                if f.len() != 7 { return None; }
                Some(Region { key: f[0].to_owned(), role: f[1].to_owned(), path: f[2].to_owned(),
                    rect: [f[3].parse().ok()?, f[4].parse().ok()?, f[5].parse().ok()?, f[6].parse().ok()?] })
            }).collect()
    })
}
fn root() -> PathBuf {
    client::shell::assets::path("visual/generated/civilizations")
}

/// A worker retains at most two decoded source sheets. GPU textures store cropped regions only.
#[derive(Default)]
pub(super) struct Decoder {
    sheets: VecDeque<(PathBuf, image::DynamicImage)>,
}
impl Decoder {
    pub fn decode(&mut self, key: &str, asset: Asset) -> Option<egui::ColorImage> {
        let (path, region) = if key.starts_with("art:") {
            let r = regions().iter().find(|r| r.key == key && r.role == asset.name())?;
            (root().join(&r.path), Some(r.rect))
        } else {
            // Legacy game keys deliberately retain their individual files, including Trajan.
            (root().join(key).join(format!("{}-v1.png", asset.name())), None)
        };
        let source = self.source(path)?;
        let cropped = if let Some([x, y, w, h]) = region {
            if w == 0 || h == 0 || x.checked_add(w)? > source.width() || y.checked_add(h)? > source.height() {
                return None;
            }
            source.crop_imm(x, y, w, h)
        } else { source.clone() };
        let image = cropped.thumbnail(asset.max_dimension(), asset.max_dimension()).to_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize], image.as_raw()))
    }
    /// What the browser's art service is asked for to make `key`'s `asset` (None: the catalogue has no such picture).
    #[cfg(target_arch = "wasm32")]
    pub fn request(&self, key: &str, asset: Asset) -> Option<client::shell::assets::web::PictureRequest> {
        let (path, rect) = if key.starts_with("art:") {
            let r = regions().iter().find(|r| r.key == key && r.role == asset.name())?;
            (root().join(&r.path), Some(r.rect))
        } else {
            (root().join(key).join(format!("{}-v1.png", asset.name())), None)
        };
        let file = client::shell::assets::web::key(&client::shell::assets::image_file(&path));
        // The portrait and the background are what the eye goes to; the roster's emblems follow.
        let hero = asset != Asset::Emblem;
        Some(client::shell::assets::web::PictureRequest::new(file, rect, asset.max_dimension(), hero))
    }
    fn source(&mut self, path: PathBuf) -> Option<&image::DynamicImage> {
        if let Some(index) = self.sheets.iter().position(|(p, _)| p == &path) {
            let cached = self.sheets.remove(index)?;
            self.sheets.push_back(cached);
        } else {
            let image = client::shell::assets::open_image(&path)?;
            self.sheets.push_back((path, image));
            while self.sheets.len() > 2 { self.sheets.pop_front(); }
        }
        self.sheets.back().map(|(_, image)| image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_art_catalog_has_valid_unique_regions_and_keeps_augustus_distinct() {
        let cards = cards();
        assert_eq!(cards.len(), 43);
        assert_eq!(regions().len(), 129);
        let keys: std::collections::HashSet<_> = cards.iter().map(|c| &c.key).collect();
        assert_eq!(keys.len(), 43);
        assert_eq!(cards.iter().find(|c| c.key == "art:rome").unwrap().leader, "Augustus Caesar");
        let mut decoder = Decoder::default();
        for card in &cards {
            for asset in [Asset::Portrait, Asset::Emblem, Asset::Background] {
                let pixels = decoder.decode(&card.key, asset).expect("every region decodes inside its real sheet");
                assert!(pixels.size[0] > 0 && pixels.size[1] > 0);
                assert!(decoder.sheets.len() <= 2);
            }
        }
        let augustus = decoder.decode("art:rome", Asset::Portrait).unwrap();
        let trajan = decoder.decode("rome", Asset::Portrait).unwrap();
        assert_ne!(augustus.pixels, trajan.pixels);
    }
}
