#[derive(Debug)]
pub struct Branch {
    pub name: String,
    pub buildid: Option<u32>,
    pub description: Option<String>,
    pub pwdrequired: bool,
    pub timeupdated: Option<u32>,
    pub sc_schinese: Option<String>,
}

impl Branch {
    pub fn new(name: String) -> Self {
        Self {
            name,
            buildid: None,
            description: None,
            pwdrequired: false,
            timeupdated: None,
            sc_schinese: None,
        }
    }
}
