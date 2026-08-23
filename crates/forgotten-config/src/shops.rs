use super::{ConfigError, EngineConfig};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::fs;

const SHOP_CATALOG_RELATIVE_PATH: &str = "XML/forgotten-engine-shops.xml";
const MAX_SHOP_CATALOG_BYTES: usize = 128 * 1024;
const MAX_SHOP_CATALOG_DEPTH: usize = 4;
const MAX_SHOP_ENTRIES_PER_NPC: usize = 64;

/// One bounded shop entry: the authoritative server item ID plus its gold prices. A missing
/// `buy` price means the NPC refuses to stock it; a missing `sell` price means the NPC will
/// not purchase it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarativeShopEntry {
    pub server_id: u16,
    pub buy_price_gold: Option<u64>,
    pub sell_price_gold: Option<u64>,
}

/// One NPC's shop keyed by the NPC's exact materialized display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeNpcShop {
    pub npc_name: String,
    pub entries: Vec<DeclarativeShopEntry>,
}

impl DeclarativeNpcShop {
    pub fn entry(&self, server_id: u16) -> Option<DeclarativeShopEntry> {
        self.entries
            .iter()
            .find(|entry| entry.server_id == server_id)
            .copied()
    }
}

/// Validated NPC shop catalog. Immutable runtime input; no scripts, no dynamic stock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarativeShopCatalog {
    shops: BTreeMap<String, DeclarativeNpcShop>,
}

impl DeclarativeShopCatalog {
    /// Resolves one exact normalized NPC name to its declared shop.
    pub fn by_npc_name(&self, npc_name: &str) -> Option<&DeclarativeNpcShop> {
        self.shops.get(npc_name)
    }

    pub fn len(&self) -> usize {
        self.shops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shops.is_empty()
    }

    fn insert(&mut self, shop: DeclarativeNpcShop) -> Result<(), ConfigError> {
        if self.shops.insert(shop.npc_name.clone(), shop).is_some() {
            return Err(invalid("duplicate NPC shop declaration"));
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

fn xml_error(error: quick_xml::Error) -> ConfigError {
    invalid(format!("invalid shop XML: {error}"))
}

/// Loads the optional operator NPC shop catalog from
/// `data/XML/forgotten-engine-shops.xml`. A missing file yields no shops.
pub fn load_declarative_shop_catalog(
    config: &EngineConfig,
) -> Result<Option<DeclarativeShopCatalog>, ConfigError> {
    let path = config.content_directory.join(SHOP_CATALOG_RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    parse_declarative_shops_xml(&fs::read(path).map_err(ConfigError::Io)?).map(Some)
}

pub fn parse_declarative_shops_xml(bytes: &[u8]) -> Result<DeclarativeShopCatalog, ConfigError> {
    if bytes.len() > MAX_SHOP_CATALOG_BYTES {
        return Err(invalid("shop catalog exceeds the configured size limit"));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut active_npc: Option<DeclarativeNpcShop> = None;
    let mut catalog = DeclarativeShopCatalog::default();
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_SHOP_CATALOG_DEPTH {
                    return Err(invalid("shop XML nesting exceeds the configured limit"));
                }
                if depth == 1 {
                    if root_seen || event.name().as_ref() != b"fe-shops" {
                        return Err(invalid("shop root element is invalid"));
                    }
                    root_seen = true;
                } else if depth == 2 && event.name().as_ref() == b"fe-shop" {
                    let npc_name = required_string(&event, b"npc")?;
                    active_npc = Some(DeclarativeNpcShop {
                        npc_name,
                        entries: Vec::new(),
                    });
                } else {
                    return Err(invalid("unexpected shop XML element"));
                }
            }
            Event::Empty(event) => {
                if depth != 2 || event.name().as_ref() != b"fe-item" {
                    return Err(invalid("unexpected shop item element"));
                }
                let Some(shop) = active_npc.as_mut() else {
                    return Err(invalid("shop item outside an NPC shop element"));
                };
                if shop.entries.len() >= MAX_SHOP_ENTRIES_PER_NPC {
                    return Err(invalid("shop entries exceed the supported bound per NPC"));
                }
                shop.entries.push(parse_shop_entry(&event)?);
            }
            Event::End(event) => {
                if depth == 0 {
                    return Err(invalid("unexpected closing XML element"));
                }
                if depth == 2 && event.name().as_ref() == b"fe-shop" {
                    let Some(shop) = active_npc.take() else {
                        return Err(invalid("shop closing tag without an opening element"));
                    };
                    catalog.insert(shop)?;
                } else if depth == 1 && event.name().as_ref() != b"fe-shops" {
                    return Err(invalid("shop root closing tag is invalid"));
                }
                depth -= 1;
            }
            Event::Text(text) if !text.as_ref().iter().all(u8::is_ascii_whitespace) => {
                return Err(invalid("shop catalog cannot contain text nodes"));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return Err(invalid("unsupported shop XML node")),
        }
        buffer.clear();
    }
    if depth != 0 || !root_seen || active_npc.is_some() {
        return Err(invalid("shop catalog is missing a complete root"));
    }
    Ok(catalog)
}

fn required_string(event: &BytesStart<'_>, key: &[u8]) -> Result<String, ConfigError> {
    let attribute = event
        .attributes()
        .with_checks(false)
        .find_map(|attribute| {
            let attribute = attribute.ok()?;
            (attribute.key.as_ref() == key).then_some(attribute)
        })
        .ok_or_else(|| invalid("shop element is missing a required attribute"))?;
    let value = attribute
        .unescape_value()
        .map_err(|error| invalid(format!("invalid shop attribute: {error}")))?;
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(invalid("shop NPC name must stay within the bounded length"));
    }
    Ok(trimmed.to_owned())
}

fn parse_shop_entry(event: &BytesStart<'_>) -> Result<DeclarativeShopEntry, ConfigError> {
    let mut known = [false; 3];
    let mut server_id = None;
    let mut buy_price = None;
    let mut sell_price = None;
    for attribute in event.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid shop attribute: {error}")))?;
        match attribute.key.as_ref() {
            b"id" => {
                let value = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid shop id: {error}")))?;
                server_id =
                    Some(value.parse::<u16>().map_err(|_| {
                        invalid("shop item id must fit an unsigned 16-bit integer")
                    })?);
                known[0] = true;
            }
            b"buy" => {
                let value = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid shop price: {error}")))?;
                buy_price =
                    Some(value.parse::<u64>().map_err(|_| {
                        invalid("shop buy price must fit an unsigned 64-bit integer")
                    })?);
                known[1] = true;
            }
            b"sell" => {
                let value = attribute
                    .unescape_value()
                    .map_err(|error| invalid(format!("invalid shop price: {error}")))?;
                sell_price =
                    Some(value.parse::<u64>().map_err(|_| {
                        invalid("shop sell price must fit an unsigned 64-bit integer")
                    })?);
                known[2] = true;
            }
            _ => return Err(invalid("unsupported shop attribute")),
        }
    }
    let Some(server_id) = server_id.filter(|id| *id != 0) else {
        return Err(invalid("shop item id must be nonzero"));
    };
    if !known[1] && !known[2] {
        return Err(invalid("shop entry needs a buy or sell price"));
    }
    Ok(DeclarativeShopEntry {
        server_id,
        buy_price_gold: buy_price,
        sell_price_gold: sell_price,
    })
}
