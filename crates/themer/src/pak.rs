use std::{collections::BTreeMap, io::Read};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::GzDecoder;

const INDEX_ENTRY_BYTES: usize = 6;

#[derive(Debug)]
pub struct Pak {
    version: u32,
    encoding: u8,
    pub resources: BTreeMap<u16, Vec<u8>>,
    aliases: BTreeMap<u16, u16>,
}

impl Pak {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() >= 9, "Truncated PAK header");
        let version = u32_at(bytes, 0)?;
        let (encoding, count, alias_count, header) = match version {
            4 => (bytes[8], u32_at(bytes, 4)? as usize, 0, 9),
            5 => {
                ensure!(bytes.len() >= 12, "Truncated PAK v5 header");
                (
                    bytes[4],
                    u16_at(bytes, 8)? as usize,
                    u16_at(bytes, 10)? as usize,
                    12,
                )
            }
            _ => bail!("Unsupported PAK version {version}; expected 4 or 5"),
        };
        ensure!(encoding <= 2, "Unknown PAK encoding {encoding}");
        ensure!(count <= u16::MAX as usize, "Too many PAK resources");
        let index_end = header + (count + 1) * INDEX_ENTRY_BYTES;
        let payload_start = index_end + alias_count * 4;
        ensure!(payload_start <= bytes.len(), "Truncated PAK index");
        let mut resources = BTreeMap::new();
        let mut ids = Vec::with_capacity(count);
        let mut previous_id = None;
        let mut offset = u32_at(bytes, header + 2)? as usize;
        ensure!(offset == payload_start, "Invalid PAK payload offset");
        for i in 0..count {
            let id = u16_at(bytes, header + i * INDEX_ENTRY_BYTES)?;
            ensure!(
                previous_id.is_none_or(|prev| id > prev),
                "Unsorted PAK resource IDs"
            );
            let end = u32_at(bytes, header + (i + 1) * INDEX_ENTRY_BYTES + 2)? as usize;
            ensure!(
                end >= offset && end <= bytes.len(),
                "Invalid PAK resource bounds for {id}"
            );
            resources.insert(id, bytes[offset..end].to_vec());
            ids.push(id);
            previous_id = Some(id);
            offset = end;
        }
        ensure!(offset == bytes.len(), "Unexpected bytes after PAK payload");
        let mut aliases = BTreeMap::new();
        let mut previous_alias = None;
        for i in 0..alias_count {
            let pos = index_end + i * 4;
            let id = u16_at(bytes, pos)?;
            let index = u16_at(bytes, pos + 2)? as usize;
            ensure!(index < count, "Invalid PAK alias index");
            ensure!(
                !resources.contains_key(&id),
                "PAK alias collides with resource {id}"
            );
            ensure!(
                previous_alias.is_none_or(|prev| id > prev),
                "Unsorted PAK aliases"
            );
            aliases.insert(id, ids[index]);
            previous_alias = Some(id);
        }
        Ok(Self {
            version,
            encoding,
            resources,
            aliases,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let count = u16::try_from(self.resources.len()).context("Too many PAK resources")?;
        let alias_count = u16::try_from(self.aliases.len()).context("Too many PAK aliases")?;
        let header = if self.version == 4 { 9 } else { 12 };
        let mut offset = header + (usize::from(count) + 1) * 6 + usize::from(alias_count) * 4;
        let mut bytes = self.version.to_le_bytes().to_vec();
        if self.version == 4 {
            bytes.extend(u32::from(count).to_le_bytes());
            bytes.push(self.encoding);
        } else {
            bytes.extend([self.encoding, 0, 0, 0]);
            bytes.extend(count.to_le_bytes());
            bytes.extend(alias_count.to_le_bytes());
        }
        for (id, payload) in &self.resources {
            bytes.extend(id.to_le_bytes());
            bytes.extend(u32::try_from(offset)?.to_le_bytes());
            offset = offset
                .checked_add(payload.len())
                .context("PAK is too large")?;
        }
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(u32::try_from(offset)?.to_le_bytes());
        let indices: BTreeMap<_, _> = self
            .resources
            .keys()
            .enumerate()
            .map(|(i, id)| (*id, i as u16))
            .collect();
        for (id, target) in &self.aliases {
            bytes.extend(id.to_le_bytes());
            bytes.extend(indices[target].to_le_bytes());
        }
        for payload in self.resources.values() {
            bytes.extend(payload);
        }
        Ok(bytes)
    }

    pub fn replace(&mut self, id: u16, payload: Vec<u8>) {
        // An alias must keep its old content when only the original resource changes.
        let former = self.resources[&id].clone();
        let detached: Vec<_> = self
            .aliases
            .iter()
            .filter(|(_, target)| **target == id)
            .map(|(alias, _)| *alias)
            .collect();
        for alias in detached {
            self.aliases.remove(&alias);
            self.resources.insert(alias, former.clone());
        }
        self.resources.insert(id, payload);
    }

    pub fn count(&self) -> usize {
        self.resources.len() + self.aliases.len()
    }
}

pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        GzDecoder::new(bytes).read_to_end(&mut decoded)?;
    } else if bytes.starts_with(&[0x1e, 0x9b]) {
        ensure!(bytes.len() >= 8, "Truncated Brotli resource header");
        let mut size = [0_u8; 8];
        size[..6].copy_from_slice(&bytes[2..8]);
        let expected = u64::from_le_bytes(size);
        brotli::Decompressor::new(&bytes[8..], 4096).read_to_end(&mut decoded)?;
        ensure!(
            decoded.len() as u64 == expected,
            "Brotli resource size mismatch"
        );
    } else {
        return Ok(bytes.to_vec());
    }
    Ok(decoded)
}

fn u16_at(bytes: &[u8], pos: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(pos..pos + 2)
            .context("Truncated PAK integer")?
            .try_into()?,
    ))
}

fn u32_at(bytes: &[u8], pos: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(pos..pos + 4)
            .context("Truncated PAK integer")?
            .try_into()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_v5_archive_retains_aliases_and_encoding() {
        let bytes = [
            5, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 7, 0, 28, 0, 0, 0, 0, 0, 31, 0, 0, 0, 9, 0, 0, 0,
            b'a', b'b', b'c',
        ];
        assert_eq!(Pak::parse(&bytes).unwrap().encode().unwrap(), bytes);
    }

    #[test]
    fn replacing_an_aliased_resource_keeps_the_alias_content() {
        let bytes = [
            5, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 7, 0, 28, 0, 0, 0, 0, 0, 31, 0, 0, 0, 9, 0, 0, 0,
            b'a', b'b', b'c',
        ];
        let mut pak = Pak::parse(&bytes).unwrap();
        pak.replace(7, b"new".to_vec());
        let reopened = Pak::parse(&pak.encode().unwrap()).unwrap();
        assert_eq!(reopened.resources[&9], b"abc");
    }

    #[test]
    fn a_resource_offset_outside_the_archive_is_rejected() {
        let bytes = [
            5, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 7, 0, 24, 0, 0, 0, 0, 0, 255, 255, 255, 255,
        ];
        assert!(
            Pak::parse(&bytes)
                .unwrap_err()
                .to_string()
                .contains("bounds")
        );
    }

    #[test]
    fn a_truncated_compressed_resource_is_rejected() {
        assert!(decode(&[0x1e, 0x9b, 0]).is_err());
    }
}
