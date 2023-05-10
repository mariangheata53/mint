use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{
    BlobCache, BlobRef, Cache, Mod, ModProvider, ModProviderCache, ModResolution, ModResponse,
    ModSpecification, ResolvableStatus,
};
use crate::config::ConfigWrapper;

inventory::submit! {
    super::ProviderFactory {
        id: "http",
        new: HttpProvider::new_provider,
        can_provide: |spec| -> bool {
            RE_MOD
                .captures(&spec.url)
                .and_then(|c| c.name("hostname"))
                .map_or(false, |h| {
                    !["mod.io", "drg.mod.io", "drg.old.mod.io"].contains(&h.as_str())
                })
        },
        parameters: &[],
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpProviderCache {
    url_blobs: HashMap<String, BlobRef>,
}
#[typetag::serde]
impl ModProviderCache for HttpProviderCache {
    fn new() -> Self {
        Default::default()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
pub struct HttpProvider {
    client: reqwest::Client,
}

impl HttpProvider {
    pub fn new_provider(_parameters: &HashMap<String, String>) -> Result<Box<dyn ModProvider>> {
        Ok(Box::new(Self::new()))
    }
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

lazy_static::lazy_static! {
    static ref RE_MOD: regex::Regex = regex::Regex::new(r"^https?://(?P<hostname>[^/]+)(/|$)").unwrap();
}

const HTTP_PROVIDER_ID: &str = "http";

#[async_trait::async_trait]
impl ModProvider for HttpProvider {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        _update: bool,
        _cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        Ok(ModResponse::Resolve(Mod {
            spec: spec.clone(),
            status: ResolvableStatus::Resolvable(ModResolution {
                url: spec.url.to_owned(),
            }),
            suggested_require: false,
            suggested_dependencies: vec![],
        }))
    }

    async fn fetch_mod(
        &self,
        url: &str,
        update: bool,
        cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        blob_cache: &BlobCache,
    ) -> Result<PathBuf> {
        Ok(
            if let Some(path) = if update {
                None
            } else {
                cache
                    .read()
                    .unwrap()
                    .get::<HttpProviderCache>(HTTP_PROVIDER_ID)
                    .and_then(|c| c.url_blobs.get(url))
                    .and_then(|r| blob_cache.get_path(r))
            } {
                path
            } else {
                println!("downloading mod {url}...");
                let res = self.client.get(url).send().await?.error_for_status()?;
                if let Some(mime) = res
                    .headers()
                    .get(reqwest::header::HeaderName::from_static("content-type"))
                {
                    let content_type = &mime.to_str()?;
                    if !["application/zip", "application/octet-stream"].contains(content_type) {
                        return Err(anyhow!("unexpected content-type: {content_type}"));
                    }
                }

                let data = res.bytes().await?.to_vec();
                let blob = blob_cache.write(&data)?;
                let path = blob_cache.get_path(&blob).unwrap();
                cache
                    .write()
                    .unwrap()
                    .get_mut::<HttpProviderCache>(HTTP_PROVIDER_ID)
                    .url_blobs
                    .insert(url.to_owned(), blob);

                path
            },
        )
    }
}
