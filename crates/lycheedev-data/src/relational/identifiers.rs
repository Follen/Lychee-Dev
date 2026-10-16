//! Identifier components are values, never dot-delimited paths. SQL rendering
//! is confined to public source-map keys; field resolution uses typed parts.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ColumnRef {
    pub qualifier: String,
    pub name: String,
}
impl ColumnRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            qualifier: String::new(),
            name: name.into(),
        }
    }
    pub fn qualified(qualifier: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            qualifier: qualifier.into(),
            name: name.into(),
        }
    }
    pub fn label(&self) -> String {
        if self.qualifier.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.qualifier, self.name)
        }
    }
}
/// Raw Source.columns remain raw. A joined/compound schema carries owners in
/// a separate, equally indexed vector, so neither dots nor quote characters
/// in a field or alias need escaping or reparsing during execution.
#[derive(Clone, Copy)]
pub(super) enum ColumnOwners<'a> {
    Single(&'a str),
    PerColumn(&'a [String]),
}
impl<'a> ColumnOwners<'a> {
    pub fn owner(self, index: usize) -> &'a str {
        match self {
            Self::Single(alias) => alias,
            Self::PerColumn(owners) => &owners[index],
        }
    }
    pub fn output(self) -> Self {
        match self {
            Self::Single(_) => self,
            Self::PerColumn(_) => Self::Single(""),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RelationName {
    pub catalog: Option<String>,
    pub name: String,
    key: String,
}
/// Physical providers receive components, plus the exact reversible map key.
/// A literal dot in either component never changes the selected catalog.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceTableUse<'a> {
    pub catalog: &'a str,
    pub name: &'a str,
    pub key: &'a str,
}
fn identifier_key(value: &str) -> String {
    use sqlparser::dialect::Dialect;
    let dialect = syntax::QueryDialect::default();
    let mut chars = value.chars();
    if chars.next().is_some_and(|c| dialect.is_identifier_start(c))
        && chars.all(|c| dialect.is_identifier_part(c))
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
}
pub(super) fn lexical_key(name: &str) -> String {
    identifier_key(&identifier_lower(name))
}
impl RelationName {
    fn new(catalog: Option<String>, name: String) -> Self {
        let key = if let Some(catalog) = &catalog {
            format!("{}.{}", identifier_key(catalog), identifier_key(&name))
        } else {
            identifier_key(&name)
        };
        Self { catalog, name, key }
    }
    pub fn internal(name: &str) -> Self {
        Self {
            catalog: None,
            name: name.into(),
            key: name.into(),
        }
    }
    pub fn from_object(object: &ObjectName) -> Result<Self> {
        let parts = object
            .0
            .iter()
            .map(|part| match part {
                ObjectNamePart::Identifier(name) => Ok(syntax::identifier(name)?.to_owned()),
                _ => Err(unsupported()),
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_parts(parts)
    }
    fn from_parts(mut parts: Vec<String>) -> Result<Self> {
        if !(1..=2).contains(&parts.len()) || parts.iter().any(|s| s.is_empty() || s.contains('\0'))
        {
            return Err(binding());
        }
        let name = parts.pop().ok_or_else(binding)?;
        Ok(Self::new(parts.pop(), name))
    }
    pub fn from_key(key: &str) -> Result<Self> {
        // Preserve the public raw-name convention when no qualification or
        // SQL quote needs decoding. Compound keys use tokenizer identifiers,
        // never split_once: quoted periods belong to their component.
        if !key.contains(['.', '"', '`', '[', ']']) {
            return Self::from_parts(vec![key.to_owned()]);
        }
        use sqlparser::tokenizer::{Token, Tokenizer};
        let tokens = Tokenizer::new(&syntax::QueryDialect::default(), key)
            .tokenize()
            .map_err(|_| binding())?;
        let tokens: Vec<_> = tokens
            .into_iter()
            .filter(|t| !matches!(t, Token::Whitespace(_)))
            .collect();
        let parts = match tokens.as_slice() {
            [Token::Word(name)] => vec![name.value.clone()],
            [Token::Word(catalog), Token::Period, Token::Word(name)] => {
                vec![catalog.value.clone(), name.value.clone()]
            }
            _ => return Err(binding()),
        };
        Self::from_parts(parts)
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn label(&self) -> String {
        self.catalog.as_ref().map_or_else(
            || self.name.clone(),
            |catalog| format!("{catalog}.{}", self.name),
        )
    }
    pub fn physical_key(&self) -> String {
        format!(
            "{}.{}",
            identifier_key(&identifier_lower(
                self.catalog.as_deref().unwrap_or("static")
            )),
            lexical_key(&self.name)
        )
    }
    pub fn lexical_key(&self) -> Option<String> {
        self.catalog.is_none().then(|| {
            if self.name.starts_with('\0') {
                self.name.clone()
            } else {
                lexical_key(&self.name)
            }
        })
    }
    pub fn table_use(&self) -> SourceTableUse<'_> {
        SourceTableUse {
            catalog: self.catalog.as_deref().unwrap_or("static"),
            name: &self.name,
            key: &self.key,
        }
    }
}
