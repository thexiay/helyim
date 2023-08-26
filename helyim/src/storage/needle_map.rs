use std::{
    fs::File,
    io::{BufReader, Read},
};

use bytes::Buf;
use tracing::debug;

use crate::{
    errors::Result,
    storage::{
        needle::NeedleValue,
        needle_value_map::{MemoryNeedleValueMap, NeedleValueMap},
    },
};

#[derive(Copy, Clone, Debug, Default)]
pub enum NeedleMapType {
    #[default]
    NeedleMapInMemory = 0,
}

#[derive(Default)]
struct Metric {
    maximum_file_key: u64,
    file_count: u64,
    deleted_count: u64,
    deleted_byte_count: u64,
    file_byte_count: u64,
}

pub struct NeedleMapper {
    needle_value_map: Box<dyn NeedleValueMap>,
    metric: Metric,
}

impl Default for NeedleMapper {
    fn default() -> Self {
        NeedleMapper {
            needle_value_map: Box::new(MemoryNeedleValueMap::new()),
            metric: Metric::default(),
        }
    }
}

impl NeedleMapper {
    pub fn new(kind: NeedleMapType) -> NeedleMapper {
        #[allow(unreachable_patterns)]
        match kind {
            NeedleMapType::NeedleMapInMemory => NeedleMapper {
                needle_value_map: Box::new(MemoryNeedleValueMap::new()),
                metric: Metric::default(),
            },
            _ => panic!("not support map type: {:?}", kind),
        }
    }

    pub fn load_idx_file(&mut self, index_file: &File) -> Result<()> {
        let mut last_offset = 0;
        let mut last_size = 0;
        walk_index_file(index_file, |key, offset, size| -> Result<()> {
            if offset > last_offset {
                last_offset = offset;
                last_size = size;
            }

            if offset > 0 {
                self.set(key, NeedleValue { offset, size });
            } else {
                self.delete(key);
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn set(&mut self, key: u64, index: NeedleValue) -> Option<NeedleValue> {
        debug!("needle map set key: {}, {:?}", key, index);
        if key > self.metric.maximum_file_key {
            self.metric.maximum_file_key = key;
        }
        self.metric.file_count += 1;
        self.metric.file_byte_count += index.size as u64;
        let old = self.needle_value_map.set(key, index);

        if let Some(n) = old {
            self.metric.deleted_count += 1;
            self.metric.deleted_byte_count += n.size as u64;
        }

        old
    }

    pub fn delete(&mut self, key: u64) -> Option<NeedleValue> {
        let deleted = self.needle_value_map.delete(key);

        if let Some(n) = deleted {
            self.metric.deleted_count += 1;
            self.metric.deleted_byte_count += n.size as u64;
        }

        debug!("needle map delete key: {} {:?}", key, deleted);
        deleted
    }

    pub fn get(&self, key: u64) -> Option<NeedleValue> {
        self.needle_value_map.get(key)
    }

    pub fn file_count(&self) -> u64 {
        self.metric.file_count
    }

    pub fn delete_count(&self) -> u64 {
        self.metric.deleted_count
    }

    pub fn deleted_byte_count(&self) -> u64 {
        self.metric.deleted_byte_count
    }

    pub fn max_file_key(&self) -> u64 {
        self.metric.maximum_file_key
    }

    pub fn content_size(&self) -> u64 {
        self.metric.file_byte_count
    }
}

fn idx_entry(mut buf: &[u8]) -> (u64, u32, u32) {
    let key = buf.get_u64();
    let offset = buf.get_u32();
    let size = buf.get_u32();

    (key, offset, size)
}

// walks through index file, call fn(key, offset, size), stop with error returned by fn
pub fn walk_index_file<T>(f: &File, mut walk: T) -> Result<()>
where
    T: FnMut(u64, u32, u32) -> Result<()>,
{
    let mut reader = BufReader::new(f.try_clone()?);
    let mut buf: Vec<u8> = vec![0; 16];

    // if there is a not complete entry, will err
    for _ in 0..(f.metadata()?.len() + 15) / 16 {
        reader.read_exact(&mut buf)?;

        let (key, offset, size) = idx_entry(&buf);
        walk(key, offset, size)?;
    }

    Ok(())
}