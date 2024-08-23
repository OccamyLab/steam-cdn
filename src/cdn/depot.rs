use keyvalues_parser::Value;

use crate::error::Error;

#[derive(Debug)]
pub struct Manifest {
    pub branch: String,
    pub gid: String,
    pub size: String,
    pub download: String,
    pub encrypted: bool,
}

impl Manifest {
    pub fn gid(&self) -> Option<u64> {
        self.gid.parse::<u64>().ok()
    }
}

#[derive(Debug)]
pub struct Depot {
    pub app_id: u32,
    pub depot_id: u32,
    pub manifests: Vec<Manifest>,
}

impl Depot {
    pub fn new(app_id: u32, depot_id: u32) -> Self {
        Self {
            app_id,
            depot_id,
            manifests: Vec::new(),
        }
    }

    fn parse_manifests(&mut self, value: &[Value<'_>], r#type: &str) -> Result<(), Error> {
        if let Some(manifests_map) = value
            .get(0)
            .ok_or(Error::NoneOption)?
            .get_obj()
            .ok_or(Error::NoneOption)?
            .get(r#type)
        {
            for (key, value) in &manifests_map
                .get(0)
                .ok_or(Error::NoneOption)?
                .get_obj()
                .ok_or(Error::NoneOption)?
                .0
            {
                let data = value[0].get_obj().ok_or(Error::NoneOption)?;
                self.manifests.push(Manifest {
                    branch: key.to_string(),
                    gid: data
                        .get("gid")
                        .and_then(|v| v[0].get_str())
                        .unwrap_or_default()
                        .to_string(),
                    size: data
                        .get("size")
                        .and_then(|v| v[0].get_str())
                        .unwrap_or_default()
                        .to_string(),
                    download: data
                        .get("download")
                        .and_then(|v| v[0].get_str())
                        .unwrap_or_default()
                        .to_string(),
                    encrypted: r#type.starts_with("encrypted"),
                })
            }
        }
        Ok(())
    }

    pub fn parse(&mut self, value: &[Value<'_>]) -> Result<(), Error> {
        self.parse_manifests(value, "manifests")?;
        self.parse_manifests(value, "encryptedmanifests")?;
        Ok(())
    }
}
