//! One-shot action artwork decoder, used off the UI thread. Only one source atlas
//! is retained at a time, so its nine regions do not decode the same PNG nine times.
use super::Action;
use std::{collections::HashMap, path::PathBuf};

pub(super) struct Decoder {
    root: PathBuf,
    regions: HashMap<String, (PathBuf, [u32; 4])>,
    atlas: Option<(PathBuf, image::DynamicImage)>,
    #[cfg(test)]
    pub atlas_decodes: usize,
}
impl Default for Decoder {
    fn default() -> Self {
        let root = client::shell::assets::path("visual/generated");
        let index = client::shell::assets::read_to_string(&root.join("runtime-atlas-index.tsv")).unwrap_or_default();
        let regions = index.lines().skip(1).filter_map(|line| {
            let c: Vec<_> = line.split('\t').collect();
            if c.len() != 6 || !c[0].starts_with("art:action/") { return None; }
            Some((c[0].to_owned(), (root.join(c[1]), [c[2].parse().ok()?, c[3].parse().ok()?, c[4].parse().ok()?, c[5].parse().ok()?])))
        }).collect();
        Self { root, regions, atlas: None, #[cfg(test)] atlas_decodes: 0 }
    }
}
impl Decoder {
    pub fn decode(&mut self, action: Action) -> Option<egui::ColorImage> {
        let image = if let Some((path, [x,y,w,h])) = self.regions.get(&format!("art:action/{}", action.id())) {
            if self.atlas.as_ref().is_none_or(|(loaded, _)| loaded != path) {
                self.atlas = Some((path.clone(), client::shell::assets::open_image(path)?));
                #[cfg(test)] { self.atlas_decodes += 1; }
            }
            let atlas = &self.atlas.as_ref()?.1;
            if *w == 0 || *h == 0 || x.checked_add(*w)? > atlas.width() || y.checked_add(*h)? > atlas.height() { return None; }
            atlas.crop_imm(*x,*y,*w,*h)
        } else {
            client::shell::assets::open_image(&self.root.join(format!("actions/{}-v1.png", action.id())))?
        };
        let image = image.resize(192,192,image::imageops::FilterType::Triangle).to_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize], image.as_raw()))
    }
}
