/// Source ranges and authenticated environment records are different evidence.
/// Source values retain the existing JSON Site shape for compatibility.
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(untagged)]
pub enum ValueOrigin {
    Source(Site),
    Environment { environment:EnvironmentEvidence },
    Entry { entry:usize, slot:usize },
    Xml { xml:crate::xml_load::XmlOrigin },
}
impl ValueOrigin {
    pub fn source(&self)->Option<&Site>{match self{Self::Source(site)=>Some(site),_=>None}}
    pub fn environment(&self)->Option<&EnvironmentEvidence>{match self{Self::Environment{environment}=>Some(environment),_=>None}}
}
